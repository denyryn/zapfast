//! Translation between grammers types and ZapFast's provider-neutral model.

use grammers_client::message::Message as TgMessage;
use grammers_client::peer::{Dialog, Peer};
use grammers_client::tl;

use crate::model::{Chat, ChatKind, Content, Delivery, Message, Quoted};

/// The chat id ZapFast stores for a Telegram peer.
pub fn chat_id(peer: &Peer) -> Option<String> {
    peer.id()
        .bot_api_dialog_id()
        .map(|id| format!("tg:{id}"))
}

/// A chat row from a resolved peer, before its dialog metadata is known.
pub fn peer_chat(peer: &Peer) -> Option<Chat> {
    let id = chat_id(peer)?;
    let mut chat = Chat::new(id, peer_name(peer));
    chat.kind = match peer {
        Peer::User(_) => ChatKind::Direct,
        Peer::Group(_) | Peer::Community(_) => ChatKind::Group,
        Peer::Channel(_) => ChatKind::Broadcast,
    };
    Some(chat)
}

/// A dialog as a chat row.
pub fn dialog_chat(dialog: &Dialog) -> Option<Chat> {
    let peer = dialog.peer();
    let mut chat = peer_chat(peer)?;
    if let tl::enums::Dialog::Dialog(raw) = &dialog.raw {
        chat.pinned = raw.pinned;
        chat.unread = raw.unread_count.max(0) as u32;
        chat.marked_unread = raw.unread_mark;
        chat.archived = raw.folder_id.is_some_and(|id| id != 0);
        chat.muted_until = mute_end(&raw.notify_settings);
    }
    if let Some(last) = &dialog.last_message {
        chat.last_activity = last.date().as_second();
        chat.last = Some(last_message(last));
    }
    Some(chat)
}

/// A message as an archived row. Text-only for M2: media without a caption
/// becomes a plain placeholder instead of being downloaded.
pub fn message_row(message: &TgMessage) -> Option<Message> {
    let peer = message.peer_id();
    let chat = format!("tg:{}", peer.bot_api_dialog_id()?);
    let sender = message
        .sender()
        .and_then(|sender| sender.id().bot_api_dialog_id())
        .map(|id| format!("tg:{id}"));
    let from_me = message.outgoing();
    let text = message.text();
    let content = if text.is_empty() && message.media().is_some() {
        Content::Text {
            text: "Media".to_owned(),
            preview: None,
        }
    } else {
        Content::Text {
            text: text.to_owned(),
            preview: None,
        }
    };
    Some(Message {
        id: message.id().to_string(),
        chat,
        sender: if from_me {
            "me".to_owned()
        } else {
            sender.unwrap_or_default()
        },
        sender_name: message
            .sender()
            .filter(|_| !from_me)
            .map(peer_name)
            .filter(|name| !name.is_empty()),
        from_me,
        timestamp: message.date().as_second(),
        history_order: Some(i64::from(message.id())),
        content,
        status: if from_me {
            Delivery::Sent
        } else {
            Delivery::None
        },
        delivered_at: None,
        read_at: None,
        quoted: message.reply_to_message_id().map(|id| Quoted {
            chat: None,
            id: id.to_string(),
            sender: String::new(),
            sender_name: None,
            summary: String::new(),
            labelled: Some(false),
            mentions: Vec::new(),
        }),
        reactions: Vec::new(),
        edited: message.edit_date().is_some(),
        mentions: Vec::new(),
        forwarded: false,
        thumbnail: None,
    })
}

pub fn peer_name(peer: &Peer) -> String {
    match peer {
        Peer::User(user) => user.full_name(),
        Peer::Group(group) => group.title().unwrap_or_default().to_owned(),
        Peer::Channel(channel) => channel.title().to_owned(),
        Peer::Community(community) => community.title().to_owned(),
    }
}

pub fn last_message(message: &TgMessage) -> crate::model::LastMessage {
    let text = message.text();
    let summary = if text.is_empty() {
        "Media".to_owned()
    } else {
        text.to_owned()
    };
    crate::model::LastMessage {
        from_me: message.outgoing(),
        sender: message
            .sender()
            .and_then(|sender| sender.id().bot_api_dialog_id())
            .map(|id| format!("tg:{id}"))
            .unwrap_or_default(),
        sender_name: message
            .sender()
            .filter(|_| !message.outgoing())
            .map(peer_name),
        full: summary.clone(),
        summary,
        labelled: text.is_empty(),
        status: if message.outgoing() {
            Delivery::Sent
        } else {
            Delivery::None
        },
    }
}

fn mute_end(settings: &tl::enums::PeerNotifySettings) -> Option<i64> {
    let tl::enums::PeerNotifySettings::Settings(settings) = settings;
    match settings.mute_until {
        Some(i32::MAX) => Some(0),
        Some(until) if i64::from(until) > crate::util::now() => Some(i64::from(until)),
        _ => None,
    }
}

