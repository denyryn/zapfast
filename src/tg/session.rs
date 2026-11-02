//! Grammers session state kept in the account's encrypted `session.db`.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::sync::Mutex;

use grammers_client::session::types::{
    ChannelState, DcOption, PeerId, PeerInfo, UpdateState, UpdatesState,
};
use grammers_client::session::{BoxFuture, Session, SessionData};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

const VERSION: i64 = 1;

#[derive(Serialize, Deserialize)]
struct Snapshot {
    home_dc: i32,
    dc_options: HashMap<i32, DcOption>,
    peer_infos: HashMap<PeerId, PeerInfo>,
    updates_state: UpdatesState,
}

impl Snapshot {
    fn of(data: &SessionData) -> Self {
        Self {
            home_dc: data.home_dc,
            dc_options: data.dc_options.clone(),
            peer_infos: data.peer_infos.clone(),
            updates_state: data.updates_state.clone(),
        }
    }

    fn into_data(self) -> SessionData {
        SessionData {
            home_dc: self.home_dc,
            dc_options: self.dc_options,
            peer_infos: self.peer_infos,
            updates_state: self.updates_state,
        }
    }
}

#[derive(Debug)]
pub enum SessionError {
    Lock,
    Sql(rusqlite::Error),
    Json(serde_json::Error),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lock => f.write_str("the Telegram session lock was poisoned"),
            Self::Sql(error) => write!(f, "the Telegram session database failed: {error}"),
            Self::Json(error) => write!(f, "the Telegram session state is unreadable: {error}"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<rusqlite::Error> for SessionError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sql(error)
    }
}

impl From<serde_json::Error> for SessionError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// A [`Session`] that writes its whole state to one row after every change.
pub struct StoredSession {
    data: Mutex<SessionData>,
    db: Mutex<Connection>,
}

impl StoredSession {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        Self::with_connection(crate::archive::open_telegram(path)?)
    }

    fn with_connection(db: Connection) -> anyhow::Result<Self> {
        db.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS tg_session (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 version INTEGER NOT NULL,
                 data TEXT NOT NULL
             );",
        )?;
        let stored: Option<(i64, String)> = db
            .query_row("SELECT version, data FROM tg_session WHERE id = 1", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()?;
        let data = match stored {
            Some((VERSION, json)) => serde_json::from_str::<Snapshot>(&json)?.into_data(),
            Some((version, _)) => anyhow::bail!("unsupported Telegram session version {version}"),
            None => SessionData::default(),
        };
        Ok(Self {
            data: Mutex::new(data),
            db: Mutex::new(db),
        })
    }

    fn data(&self) -> Result<std::sync::MutexGuard<'_, SessionData>, SessionError> {
        self.data.lock().map_err(|_| SessionError::Lock)
    }

    fn persist(&self, data: &SessionData) -> Result<(), SessionError> {
        let json = serde_json::to_string(&Snapshot::of(data))?;
        let db = self.db.lock().map_err(|_| SessionError::Lock)?;
        db.execute(
            "INSERT INTO tg_session (id, version, data) VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET version = excluded.version, data = excluded.data",
            params![VERSION, json],
        )?;
        Ok(())
    }

    pub fn clear(&self) -> Result<(), SessionError> {
        let mut data = self.data()?;
        *data = SessionData::default();
        self.persist(&data)
    }
}

impl Session for StoredSession {
    type Error = SessionError;

    fn home_dc_id(&self) -> Result<i32, Self::Error> {
        Ok(self.data()?.home_dc)
    }

    fn set_home_dc_id(&self, dc_id: i32) -> BoxFuture<'_, Result<(), Self::Error>> {
        Box::pin(async move {
            let mut data = self.data()?;
            data.home_dc = dc_id;
            self.persist(&data)
        })
    }

    fn dc_option(&self, dc_id: i32) -> Result<Option<DcOption>, Self::Error> {
        Ok(self.data()?.dc_options.get(&dc_id).cloned())
    }

    fn set_dc_option(&self, option: &DcOption) -> BoxFuture<'_, Result<(), Self::Error>> {
        let option = option.clone();
        Box::pin(async move {
            let mut data = self.data()?;
            data.dc_options.insert(option.id, option);
            self.persist(&data)
        })
    }

    fn peer(&self, peer: PeerId) -> BoxFuture<'_, Result<Option<PeerInfo>, Self::Error>> {
        Box::pin(async move { Ok(self.data()?.peer_infos.get(&peer).cloned()) })
    }

    fn cache_peer(&self, peer: PeerInfo) -> BoxFuture<'_, Result<(), Self::Error>> {
        Box::pin(async move {
            let mut data = self.data()?;
            data.peer_infos.insert(peer.id(), peer);
            self.persist(&data)
        })
    }

    fn cache_peers(&self, peers: Vec<PeerInfo>) -> BoxFuture<'_, Result<(), Self::Error>> {
        Box::pin(async move {
            if peers.is_empty() {
                return Ok(());
            }
            let mut data = self.data()?;
            for peer in peers {
                data.peer_infos.insert(peer.id(), peer);
            }
            self.persist(&data)
        })
    }

    fn updates_state(&self) -> BoxFuture<'_, Result<UpdatesState, Self::Error>> {
        Box::pin(async move { Ok(self.data()?.updates_state.clone()) })
    }

    fn set_update_state(&self, update: UpdateState) -> BoxFuture<'_, Result<(), Self::Error>> {
        Box::pin(async move {
            let mut data = self.data()?;
            match update {
                UpdateState::All(state) => data.updates_state = state,
                UpdateState::Primary { pts, date, seq } => {
                    data.updates_state.pts = pts;
                    data.updates_state.date = date;
                    data.updates_state.seq = seq;
                }
                UpdateState::Secondary { qts } => data.updates_state.qts = qts,
                UpdateState::Channel { id, pts } => {
                    data.updates_state.channels.retain(|c| c.id != id);
                    data.updates_state.channels.push(ChannelState { id, pts });
                }
            }
            self.persist(&data)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(path: &Path) -> StoredSession {
        StoredSession::with_connection(Connection::open(path).unwrap()).unwrap()
    }

    fn block<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    #[test]
    fn a_session_survives_a_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.db");
        let option = DcOption {
            id: 4,
            ipv4: "149.154.167.91:443".parse().unwrap(),
            ipv6: "[2001:67c:4e8:f004::9]:443".parse().unwrap(),
            auth_key: Some([7; 256]),
        };
        {
            let session = stored(&path);
            block(session.set_home_dc_id(4)).unwrap();
            block(session.set_dc_option(&option)).unwrap();
            block(session.cache_peer(PeerInfo::User {
                id: 42,
                auth: None,
                bot: Some(false),
                is_self: Some(false),
            }))
            .unwrap();
            block(session.set_update_state(UpdateState::Primary {
                pts: 9,
                date: 10,
                seq: 11,
            }))
            .unwrap();
        }

        let session = stored(&path);
        assert_eq!(session.home_dc_id().unwrap(), 4);
        assert_eq!(session.dc_option(4).unwrap(), Some(option));
        assert_eq!(
            block(session.peer(PeerId::user(42).unwrap())).unwrap(),
            Some(PeerInfo::User {
                id: 42,
                auth: None,
                bot: Some(false),
                is_self: Some(false),
            })
        );
        let state = block(session.updates_state()).unwrap();
        assert_eq!((state.pts, state.date, state.seq), (9, 10, 11));
    }

    #[test]
    fn unreadable_state_is_refused_instead_of_reset() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.db");
        {
            let db = Connection::open(&path).unwrap();
            db.execute_batch(
                "CREATE TABLE tg_session (id INTEGER PRIMARY KEY, version INTEGER NOT NULL, data TEXT NOT NULL);
                 INSERT INTO tg_session (id, version, data) VALUES (1, 1, 'not json');",
            )
            .unwrap();
        }
        assert!(StoredSession::with_connection(Connection::open(&path).unwrap()).is_err());
    }
}
