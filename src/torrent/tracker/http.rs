use super::{PeerInfo, TorrentState};
use crate::metainfo::Metainfo;
use failure::{self, Fail};
use log::debug;
use reqwest::{self, Client, Method, Url};
use serde_derive::{Deserialize, Serialize};
use serde_urlencoded;
use std::sync::Arc;
use url::percent_encoding::{percent_encode, DEFAULT_ENCODE_SET};

const DEFAULT_NUM_PEERS: u64 = 30;

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum Event {
    Started,
    Completed,
    Stopped,
}

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "query serialization error: {}", _0)]
    QuerySerialize(#[fail(cause)] serde_urlencoded::ser::Error),
    #[fail(display = "url serialization error: {}", _0)]
    URLSerialize(#[fail(cause)] reqwest::UrlError),
    #[fail(display = "response deserialization error: {}", _0)]
    Deserialize(#[fail(cause)] serde_bencode::error::Error),
    #[fail(display = "reqwest error: {}", _0)]
    Reqwest(#[fail(cause)] reqwest::Error),
    #[fail(display = "tracker error: {}", _0)]
    Tracker(String),
}

impl From<serde_urlencoded::ser::Error> for Error {
    fn from(e: serde_urlencoded::ser::Error) -> Self {
        Error::QuerySerialize(e)
    }
}

impl From<reqwest::UrlError> for Error {
    fn from(e: reqwest::UrlError) -> Self {
        Error::URLSerialize(e)
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error::Reqwest(e)
    }
}

impl From<serde_bencode::error::Error> for Error {
    fn from(e: serde_bencode::error::Error) -> Self {
        Error::Deserialize(e)
    }
}

#[derive(Serialize)]
struct Request<'a, 'b, 'c> {
    #[serde(skip)]
    base: Url,
    #[serde(skip)]
    info_hash: &'a str,
    peer_id: &'b str,
    #[serde(rename = "trackerid")]
    tracker_id: Option<&'b str>,
    port: u16,
    #[serde(flatten)]
    torrent_state: &'c TorrentState,
    #[serde(rename = "numwant")]
    num_peers: u64,
    event: Option<Event>,
}

impl Request<'_, '_, '_> {
    fn into_url(self) -> Result<Url, Error> {
        debug!("Building Request URL");
        let mut query = serde_urlencoded::to_string(&self)?;
        query.push_str("&info_hash=");
        query.push_str(self.info_hash);
        let mut base = self.base;
        base.set_query(Some(&query));
        Ok(base)
    }
}

#[derive(Deserialize, Debug)]
struct Valid {
    #[serde(rename = "warning message")]
    warning_message: Option<String>,
    interval: u64,
    #[serde(rename = "tracker id")]
    tracker_id: Option<String>,
    peers: Vec<PeerInfo>,
}

impl Valid {
    fn from_response(res: Response) -> Result<Self, Response> {
        if res.peers.is_none() || res.interval.is_none() {
            return Err(res);
        }

        Ok(Valid {
            warning_message: res.warning_message,
            interval: res.interval.unwrap(),
            tracker_id: res.tracker_id,
            peers: res.peers.unwrap(),
        })
    }
}

#[derive(Deserialize, Debug)]
struct Response {
    #[serde(rename = "failure reason")]
    failure_reason: Option<String>,
    #[serde(rename = "tracker id")]
    tracker_id: Option<String>,
    #[serde(rename = "warning message")]
    warning_message: Option<String>,
    interval: Option<u64>,
    peers: Option<Vec<PeerInfo>>,
}

pub struct HTTP<'a> {
    pub metainfo: Arc<Metainfo>,
    pub peer_id: Arc<String>,
    pub port: u16,
    pub client: &'a Client,
    announced: bool,
    tracker_id: Option<String>,
    info_hash: Option<String>,
}

impl<'a> HTTP<'a> {
    pub fn new(
        metainfo: Arc<Metainfo>,
        peer_id: Arc<String>,
        port: u16,
        client: &'a Client,
    ) -> Self {
        HTTP {
            metainfo,
            peer_id,
            port,
            client,
            announced: false,
            tracker_id: None,
            info_hash: None,
        }
    }

    pub fn get_peers(
        &mut self,
        state: &TorrentState,
        num_peers: Option<u64>,
    ) -> Result<Vec<PeerInfo>, Error> {
        if self.info_hash.is_none() {
            self.info_hash = Some(
                percent_encode(
                    &self
                        .metainfo
                        .info_hash()
                        .expect("Invalid metainfo provided"),
                    DEFAULT_ENCODE_SET,
                )
                .to_string(),
            );
        }

        let req = Request {
            base: Url::parse(&self.metainfo.announce)?,
            info_hash: self.info_hash.as_ref().unwrap(),
            peer_id: self.peer_id.as_str(),
            tracker_id: self.tracker_id.as_ref().map(|x| x.as_str()),
            port: self.port,
            torrent_state: state,
            num_peers: num_peers.unwrap_or(DEFAULT_NUM_PEERS),
            event: None,
        };

        let http_request = reqwest::Request::new(Method::GET, req.into_url()?);
        let mut http_response = self.client.execute(http_request)?.error_for_status()?;
        let s = http_response.text()?;
        let res: Response = serde_bencode::de::from_str(&s)?;
        match Valid::from_response(res) {
            Ok(v) => {
                self.tracker_id = v.tracker_id;
                Ok(v.peers)
            }
            Err(res) => Err(Error::Tracker(res.failure_reason.unwrap())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{self, mock, Matcher};
    use std::borrow::Cow;

    #[test]
    fn test_url_serialization() -> Result<(), failure::Error> {
        let info_hash = "test";
        let peer_id = "CN1";
        let tracker_id = None;
        let port = 1000;
        let torrent_state = &TorrentState {
            uploaded: 0,
            downloaded: 0,
            left: 1000,
        };
        let num_peers = 10;
        let event = Some(Event::Started);
        let req = Request {
            base: Url::parse("https://localhost/announce")?,
            info_hash,
            peer_id,
            tracker_id,
            port,
            torrent_state,
            num_peers,
            event,
        };
        let url = req.into_url()?;
        let mut pairs = url.query_pairs();
        assert_eq!(pairs.count(), 8);
        assert_eq!(
            pairs.next(),
            Some((Cow::Borrowed("peer_id"), Cow::Borrowed("CN1")))
        );
        assert_eq!(
            pairs.next(),
            Some((Cow::Borrowed("port"), Cow::Borrowed("1000")))
        );
        assert_eq!(
            pairs.next(),
            Some((Cow::Borrowed("uploaded"), Cow::Borrowed("0")))
        );
        assert_eq!(
            pairs.next(),
            Some((Cow::Borrowed("downloaded"), Cow::Borrowed("0")))
        );
        assert_eq!(
            pairs.next(),
            Some((Cow::Borrowed("left"), Cow::Borrowed("1000")))
        );
        assert_eq!(
            pairs.next(),
            Some((Cow::Borrowed("numwant"), Cow::Borrowed("10")))
        );
        assert_eq!(
            pairs.next(),
            Some((Cow::Borrowed("event"), Cow::Borrowed("started")))
        );
        assert_eq!(
            pairs.next(),
            Some((Cow::Borrowed("info_hash"), Cow::Borrowed("test")))
        );
        Ok(())
    }

    #[test]
    fn test_announce() -> Result<(), failure::Error> {
        let mut m = Metainfo::from_file("data/test.torrent")?;
        m.announce = mockito::server_url() + "/announce";
        let _mck = mock("GET", Matcher::Any)
            .with_status(200)
            .with_header("content-type", "text/plain")
            .with_body_from_file("data/test_response")
            .create();
        let r = Client::new();
        let mut h = HTTP::new(Arc::new(m), Arc::new(String::from("test")), 1000, &r);
        let v = h.get_peers(
            &TorrentState {
                downloaded: 0,
                uploaded: 0,
                left: 1000,
            },
            Some(2),
        )?;
        let mut it = v.iter();
        assert_eq!(v.len(), 2);
        assert_eq!(
            it.next(),
            Some(&PeerInfo {
                id: None,
                ip: String::from("91.64.137.190"),
                port: 51413
            })
        );
        assert_eq!(
            it.next(),
            Some(&PeerInfo {
                id: None,
                ip: String::from("92.62.63.75"),
                port: 6881
            })
        );
        Ok(())
    }
}
