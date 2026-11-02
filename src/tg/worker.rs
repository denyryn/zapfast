//! The Telegram runtime: login, dialogs, history, and text messages.

use std::collections::HashMap;
use std::sync::Arc;

use grammers_client::client::{LoginToken, PasswordToken, SignInError, UpdateStream};
use grammers_client::message::InputMessage;
use grammers_client::peer::Peer;
use grammers_client::sender::{SenderPool, SenderPoolFatHandle, UpdatesConfiguration};
use grammers_client::session::types::PeerRef;
use grammers_client::session::updates::UpdatesLike;
use grammers_client::update::{MessageDeletion, Update};
use grammers_client::{Client, InvocationError};
use tokio::sync::mpsc;

use crate::archive::Archive;
use crate::backend::{Command, Event, LinkStatus, Waker};
use crate::model::{Chat, ChatId, Delivery, Message, ReplyTarget};
use crate::paths::AccountDirs;
use crate::tg::{Auth, convert, session::StoredSession};

const HISTORY: usize = 50;
const API_ID: &str = "tg_api_id";
const API_HASH: &str = "tg_api_hash";

/// Runs one linked Telegram account until its backend shuts down.
pub async fn run(
    dirs: AccountDirs,
    events: std::sync::mpsc::Sender<Event>,
    command_rx: mpsc::UnboundedReceiver<Command>,
    waker: Waker,
) {
    let archive = match Archive::open_telegram(&dirs.telegram_db()) {
        Ok(archive) => archive,
        Err(error) => {
            log::warn!("the Telegram archive could not be opened: {error:#}");
            let _ = events.send(Event::Link(LinkStatus::Failed(error.to_string())));
            waker.wake();
            return;
        }
    };
    let session = match StoredSession::open(&dirs.state.join("session.db")) {
        Ok(session) => Arc::new(session),
        Err(error) => {
            log::warn!("the Telegram session could not be opened: {error:#}");
            let _ = events.send(Event::Link(LinkStatus::Failed(error.to_string())));
            waker.wake();
            return;
        }
    };
    let mut worker = Worker {
        dirs,
        archive,
        session,
        events,
        waker,
        client: None,
        handle: None,
        updates_rx: None,
        updates: None,
        peers: HashMap::new(),
        login_token: None,
        password_token: None,
        api_id: None,
        authorized: false,
    };
    worker.start().await;
    worker.serve(command_rx).await;
}

struct Worker {
    dirs: AccountDirs,
    archive: Archive,
    session: Arc<StoredSession>,
    events: std::sync::mpsc::Sender<Event>,
    waker: Waker,
    client: Option<Client>,
    handle: Option<SenderPoolFatHandle>,
    updates_rx: Option<mpsc::Receiver<UpdatesLike>>,
    updates: Option<UpdateStream>,
    peers: HashMap<String, PeerRef>,
    login_token: Option<LoginToken>,
    password_token: Option<PasswordToken>,
    api_id: Option<i32>,
    authorized: bool,
}

impl Worker {
    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
        self.waker.wake();
    }

    async fn start(&mut self) {
        if let Err(error) = self.dirs.ensure() {
            self.emit(Event::Link(LinkStatus::Failed(error.to_string())));
            return;
        }
        self.emit(Event::Link(LinkStatus::Connecting));
        let api_id = self
            .archive
            .meta(API_ID)
            .ok()
            .flatten()
            .and_then(|value| value.parse::<i32>().ok());
        let api_hash = self.archive.meta(API_HASH).ok().flatten();
        let (Some(api_id), Some(_)) = (api_id, api_hash.as_deref()) else {
            self.emit(Event::TelegramAuth(Auth::NeedCredentials { error: None }));
            self.emit(Event::Link(LinkStatus::Starting));
            return;
        };
        if !self.start_pool(api_id) {
            self.emit(Event::Link(LinkStatus::Failed(
                "the Telegram runtime could not start".to_owned(),
            )));
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        match client.is_authorized().await {
            Ok(true) => self.finish_login().await,
            Ok(false) => {
                self.emit(Event::TelegramAuth(Auth::NeedPhone { error: None }));
                self.emit(Event::Link(LinkStatus::Starting));
            }
            Err(error) => {
                log::warn!("Telegram authorization could not be checked: {error}");
                self.emit(Event::TelegramAuth(Auth::NeedPhone {
                    error: Some(error.to_string()),
                }));
                self.emit(Event::Link(LinkStatus::Disconnected {
                    reason: error.to_string(),
                }));
            }
        }
    }

    fn start_pool(&mut self, api_id: i32) -> bool {
        if self.client.is_some() && self.api_id == Some(api_id) {
            return true;
        }
        self.stop_pool();
        let pool = SenderPool::new(self.session.clone(), api_id);
        let runner = pool.runner;
        let handle = pool.handle;
        let updates = pool.updates;
        self.handle = Some(handle.clone());
        self.client = Some(Client::new(handle));
        self.updates_rx = Some(updates);
        self.api_id = Some(api_id);
        tokio::spawn(async move { runner.run().await });
        true
    }

    fn stop_pool(&mut self) {
        self.updates = None;
        self.updates_rx = None;
        self.client = None;
        if let Some(handle) = self.handle.take() {
            handle.quit();
        }
    }

    async fn finish_login(&mut self) {
        self.authorized = true;
        self.emit(Event::TelegramAuth(Auth::Authorized));
        self.emit(Event::Link(LinkStatus::Connected));
        let Some(client) = self.client.clone() else {
            return;
        };
        match client.get_me().await {
            Ok(user) => self.emit(Event::Me {
                id: user
                    .id()
                    .bot_api_dialog_id()
                    .map_or_else(|| "tg:me".to_owned(), |id| format!("tg:{id}")),
                lid: None,
                name: Some(user.full_name()),
                about: None,
            }),
            Err(error) => log::warn!("Telegram identity could not be read: {error}"),
        }
        if let Some(updates) = self.updates_rx.take() {
            match client
                .stream_updates(updates, UpdatesConfiguration { catch_up: true })
                .await
            {
                Ok(stream) => self.updates = Some(stream),
                Err(error) => log::warn!("Telegram updates could not start: {error}"),
            }
        }
        self.load_dialogs().await;
    }

    async fn load_dialogs(&mut self) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let mut dialogs = client.iter_dialogs();
        let mut chats = Vec::new();
        let mut contacts = Vec::new();
        loop {
            let dialog = match dialogs.next().await {
                Ok(Some(dialog)) => dialog,
                Ok(None) => break,
                Err(error) => {
                    log::warn!("Telegram dialogs could not be listed: {error}");
                    self.emit(Event::Link(LinkStatus::Disconnected {
                        reason: error.to_string(),
                    }));
                    break;
                }
            };
            let Some(chat) = convert::dialog_chat(&dialog) else {
                continue;
            };
            if let Ok(Some(reference)) = dialog.peer().to_ref().await {
                self.peers.insert(chat.id.clone(), reference);
            }
            if let Some(last) = &dialog.last_message
                && let Some(row) = convert::message_row(last)
            {
                let _ = self.archive.insert_message(&row, None);
            }
            let _ = self.archive.upsert_chat(&chat);
            let _ = self.archive.set_unread(&chat.id, chat.unread);
            if matches!(dialog.peer(), Peer::User(_)) {
                contacts.push(crate::model::Contact {
                    id: chat.id.clone(),
                    full_name: Some(chat.name.clone()),
                    push_name: Some(chat.name.clone()),
                    ..Default::default()
                });
            }
            chats.push(chat);
        }
        self.emit(Event::Chats(chats));
        self.emit(Event::Contacts(contacts));
    }

    async fn serve(&mut self, mut commands: mpsc::UnboundedReceiver<Command>) {
        loop {
            let wake = match self.updates.as_mut() {
                Some(updates) => tokio::select! {
                    command = commands.recv() => Wake::Command(command),
                    update = updates.next() => Wake::Update(update),
                },
                None => Wake::Command(commands.recv().await),
            };
            match wake {
                Wake::Command(Some(command)) => {
                    if !self.command(command).await {
                        break;
                    }
                }
                Wake::Command(None) => break,
                Wake::Update(Ok(update)) => self.update(update).await,
                Wake::Update(Err(error)) => {
                    log::warn!("Telegram updates stopped: {error}");
                    self.emit(Event::Link(LinkStatus::Disconnected {
                        reason: error.to_string(),
                    }));
                    self.updates = None;
                }
            }
        }
        self.finish();
    }

    async fn command(&mut self, command: Command) -> bool {
        match command {
            Command::TelegramRequestCode {
                api_id,
                api_hash,
                phone,
            } => self.request_code(api_id, api_hash, phone).await,
            Command::TelegramSubmitCode(code) => self.submit_code(code).await,
            Command::TelegramSubmitPassword(password) => self.submit_password(password).await,
            Command::TelegramLogout => self.logout().await,
            Command::Reconnect => self.start().await,
            Command::SendText { chat, text, quoting, .. } => {
                self.send_text(chat, text, quoting).await;
            }
            Command::LoadChat { chat, before } => self.load_chat(chat, before).await,
            Command::FetchOlder { chat, .. } => {
                self.emit(Event::Messages {
                    chat,
                    messages: Vec::new(),
                    older: true,
                    complete: true,
                });
            }
            Command::MarkRead { chat, .. } => self.mark_read(chat).await,
            Command::Shutdown => return false,
            _ => {}
        }
        true
    }

    async fn request_code(
        &mut self,
        api_id: Option<i32>,
        api_hash: Option<String>,
        phone: String,
    ) {
        let api_id = api_id
            .or_else(|| self.archive.meta(API_ID).ok().flatten()?.parse().ok());
        let api_hash = api_hash.or_else(|| self.archive.meta(API_HASH).ok().flatten());
        let (Some(api_id), Some(api_hash)) = (api_id, api_hash) else {
            self.emit(Event::TelegramAuth(Auth::NeedCredentials { error: None }));
            return;
        };
        if !self.start_pool(api_id) {
            self.emit(Event::TelegramAuth(Auth::NeedCredentials {
                error: Some("the Telegram runtime could not start".to_owned()),
            }));
            return;
        }
        let _ = self.archive.set_meta(API_ID, &api_id.to_string());
        let _ = self.archive.set_meta(API_HASH, &api_hash);
        let Some(client) = self.client.clone() else {
            return;
        };
        match client.request_login_code(&phone, &api_hash).await {
            Ok(token) => {
                self.login_token = Some(token);
                self.emit(Event::TelegramAuth(Auth::NeedCode { error: None }));
            }
            Err(error) => {
                let rejected = credentials_rejected(&error);
                let error = Some(error.to_string());
                if rejected {
                    self.emit(Event::TelegramAuth(Auth::NeedCredentials { error }));
                } else {
                    self.emit(Event::TelegramAuth(Auth::NeedPhone { error }));
                }
            }
        }
    }

    async fn submit_code(&mut self, code: String) {
        let (Some(client), Some(token)) = (self.client.clone(), self.login_token.as_ref()) else {
            self.emit(Event::TelegramAuth(Auth::NeedCode {
                error: Some("Ask for a code first.".to_owned()),
            }));
            return;
        };
        match client.sign_in(token, &code).await {
            Ok(_) => self.finish_login().await,
            Err(SignInError::PasswordRequired(token)) => {
                let hint = token.hint().map(str::to_owned);
                self.password_token = Some(token);
                self.emit(Event::TelegramAuth(Auth::NeedPassword { hint, error: None }));
            }
            Err(SignInError::InvalidCode) => {
                self.emit(Event::TelegramAuth(Auth::NeedCode {
                    error: Some("That code was not accepted.".to_owned()),
                }));
            }
            Err(error) => {
                self.emit(Event::TelegramAuth(Auth::NeedCode {
                    error: Some(error.to_string()),
                }));
            }
        }
    }

    async fn submit_password(&mut self, password: String) {
        let (Some(client), Some(token)) = (self.client.clone(), self.password_token.take()) else {
            self.emit(Event::TelegramAuth(Auth::NeedPassword {
                hint: None,
                error: Some("Ask for a code first.".to_owned()),
            }));
            return;
        };
        match client.check_password(token, password).await {
            Ok(_) => self.finish_login().await,
            Err(SignInError::InvalidPassword(token)) => {
                let hint = token.hint().map(str::to_owned);
                self.password_token = Some(token);
                self.emit(Event::TelegramAuth(Auth::NeedPassword {
                    hint,
                    error: Some("That password was not accepted.".to_owned()),
                }));
            }
            Err(error) => {
                self.emit(Event::TelegramAuth(Auth::NeedPassword {
                    hint: None,
                    error: Some(error.to_string()),
                }));
            }
        }
    }

    async fn logout(&mut self) {
        if let Some(client) = self.client.clone()
            && let Err(error) = client.sign_out().await
        {
            log::warn!("Telegram could not sign out: {error}");
        }
        self.stop_pool();
        let _ = self.session.clear();
        let _ = self.archive.clear();
        self.peers.clear();
        self.authorized = false;
        self.emit(Event::Link(LinkStatus::LoggedOut));
        self.emit(Event::TelegramAuth(Auth::NeedCredentials { error: None }));
    }

    async fn load_chat(&mut self, chat: ChatId, before: Option<crate::backend::PageKey>) {
        let Some(peer) = self.peer_of(&chat) else {
            self.emit(Event::Messages {
                chat,
                messages: Vec::new(),
                older: before.is_some(),
                complete: true,
            });
            return;
        };
        let Some(client) = self.client.clone() else {
            return;
        };
        let mut request = client.iter_messages(peer).limit(HISTORY);
        if let Some((_, id)) = &before
            && let Ok(id) = id.parse::<i32>()
        {
            request = request.offset_id(id);
        }
        let mut rows = Vec::new();
        loop {
            match request.next().await {
                Ok(Some(message)) => {
                    if let Some(row) = convert::message_row(&message) {
                        let _ = self.archive.insert_message(&row, None);
                        rows.push(row);
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    log::warn!("Telegram history could not be read: {error}");
                    break;
                }
            }
        }
        let complete = rows.len() < HISTORY;
        rows.reverse();
        self.emit(Event::Messages {
            chat,
            messages: rows,
            older: before.is_some(),
            complete,
        });
    }

    async fn send_text(&mut self, chat: ChatId, text: String, quoting: Option<ReplyTarget>) {
        let Some(peer) = self.peer_of(&chat) else {
            log::warn!("Telegram send to an unknown chat was dropped");
            return;
        };
        let Some(client) = self.client.clone() else {
            return;
        };
        let mut message = InputMessage::new().text(text.clone());
        if let Some(id) = quoting
            .as_ref()
            .and_then(|target| target.id.parse::<i32>().ok())
        {
            message = message.reply_to(Some(id));
        }
        match client.send_message(peer, message).await {
            Ok(sent) => {
                if let Some(row) = convert::message_row(&sent) {
                    let _ = self.archive.insert_message(&row, None);
                    self.emit(Event::Messages {
                        chat: row.chat.clone(),
                        messages: vec![row.clone()],
                        older: false,
                        complete: false,
                    });
                    self.bump_chat(&row).await;
                }
            }
            Err(error) => {
                log::warn!("Telegram message was not sent: {error}");
                let failed = Message {
                    id: format!("tg-failed-{}", crate::util::now()),
                    chat,
                    sender: "me".to_owned(),
                    sender_name: None,
                    from_me: true,
                    timestamp: crate::util::now(),
                    history_order: None,
                    content: crate::model::Content::Text { text, preview: None },
                    status: Delivery::Failed,
                    delivered_at: None,
                    read_at: None,
                    quoted: None,
                    reactions: Vec::new(),
                    edited: false,
                    mentions: Vec::new(),
                    forwarded: false,
                    thumbnail: None,
                };
                let _ = self.archive.insert_message(&failed, None);
                self.emit(Event::Messages {
                    chat: failed.chat.clone(),
                    messages: vec![failed.clone()],
                    older: false,
                    complete: false,
                });
                self.bump_chat(&failed).await;
            }
        }
    }

    async fn mark_read(&mut self, chat: ChatId) {
        if let Some(peer) = self.peer_of(&chat)
            && let Some(client) = self.client.clone()
            && let Err(error) = client.mark_as_read(peer).await
        {
            log::warn!("Telegram could not mark a chat read: {error}");
        }
        let _ = self.archive.set_unread(&chat, 0);
        if let Ok(Some(chat)) = self.archive.chat(&chat) {
            self.emit(Event::ChatUpdated(Box::new(chat)));
        }
    }

    async fn update(&mut self, update: Update) {
        match update {
            Update::NewMessage(message) => {
                let message = message.into_inner();
                let Some(row) = convert::message_row(&message) else {
                    return;
                };
                if self.archive.insert_message(&row, None).is_err() {
                    return;
                }
                self.emit(Event::Messages {
                    chat: row.chat.clone(),
                    messages: vec![row.clone()],
                    older: false,
                    complete: false,
                });
                self.bump_chat(&row).await;
                if !row.from_me {
                    self.emit(Event::Incoming {
                        chat: row.chat.clone(),
                        message: Box::new(row),
                    });
                }
            }
            Update::MessageEdited(message) => {
                if let Some(row) = convert::message_row(&message) {
                    let _ = self.archive.insert_message(&row, None);
                    self.emit(Event::MessageUpdated(Box::new(row)));
                }
            }
            Update::MessageDeleted(deletion) => self.delete(deletion),
            _ => {}
        }
    }

    fn delete(&mut self, deletion: MessageDeletion) {
        let Some(channel) = deletion.channel_id() else {
            return;
        };
        let Some(peer) = grammers_client::session::types::PeerId::channel(channel) else {
            return;
        };
        let Some(id) = peer.bot_api_dialog_id() else {
            return;
        };
        let chat = format!("tg:{id}");
        for message in deletion.messages() {
            if let Ok(true) = self.archive.delete_message(&chat, &message.to_string()) {
                self.emit(Event::MessageDeleted {
                    chat: chat.clone(),
                    id: message.to_string(),
                });
            }
        }
    }

    async fn bump_chat(&mut self, message: &Message) {
        let mut chat = match self.archive.chat(&message.chat) {
            Ok(Some(chat)) => chat,
            _ => match self.learn_chat(&message.chat).await {
                Some(chat) => chat,
                None => return,
            },
        };
        if !message.from_me {
            chat.unread = chat.unread.saturating_add(1);
            let _ = self.archive.set_unread(&chat.id, chat.unread);
        }
        let _ = self.archive.upsert_chat(&chat);
        self.emit(Event::ChatUpdated(Box::new(chat)));
    }

    /// Fetches a chat the dialog list did not carry, so a message from a peer
    /// with no row yet still lands in a listed chat.
    async fn learn_chat(&mut self, chat: &str) -> Option<Chat> {
        let peer = self.peer_of(chat)?;
        let client = self.client.clone()?;
        let resolved = client.resolve_peer(peer.clone()).await.ok()?;
        self.peers.insert(chat.to_owned(), peer);
        convert::peer_chat(&resolved)
    }

    fn peer_of(&self, chat: &str) -> Option<PeerRef> {
        self.peers.get(chat).cloned().or_else(|| {
            let id = chat.strip_prefix("tg:")?.parse::<i64>().ok()?;
            let peer = grammers_client::session::types::PeerId::from_bot_api_dialog_id(id)?;
            Some(peer.to_ambient_ref())
        })
    }

    fn finish(&mut self) {
        self.stop_pool();
    }
}

/// Whether the server refused the API ID or hash itself, which the user can
/// correct only by pasting another pair.
fn credentials_rejected(error: &InvocationError) -> bool {
    matches!(error, InvocationError::Rpc(rpc) if rpc.code == 401 || rpc.code == 403)
}

enum Wake {
    Command(Option<Command>),
    Update(Result<Update, InvocationError>),
}
