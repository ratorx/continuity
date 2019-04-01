use crypto::digest::Digest;
use crypto::sha1::Sha1;
use failure::{self, Fail};
use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Info {
    pub name: String,
    #[serde(rename = "piece length")]
    pub piece_length: usize,
    #[serde(with = "serde_bytes")]
    pub pieces: Vec<u8>,
    pub length: usize,
}

impl Info {
    fn validate(&self) -> Result<(), Error> {
        if self.name == "" {
            return Err(Error::InvalidName);
        } else if self.length == 0 {
            return Err(Error::ZeroLength);
        } else if self.piece_length == 0 {
            return Err(Error::ZeroPieceLength);
        }

        let mut num_pieces = self.length / self.piece_length;
        if num_pieces * self.piece_length < self.length {
            num_pieces += 1
        }

        if self.pieces.len() % 20 != 0 {
            return Err(Error::InvalidPieceArrayLength(
                "length of pieces array is not a multiple of 20".to_owned(),
            ));
        } else if num_pieces != self.pieces.len() / 20 {
            return Err(Error::InvalidPieceArrayLength(
                "number of pieces not equal to size of pieces array".to_owned(),
            ));
        }

        Ok(())
    }

    fn hash(&self) -> Result<[u8; 20], Error> {
        self.validate()?;
        let mut hash: [u8; 20] = [0; 20];
        let mut hasher = Sha1::new();
        hasher.input(&serde_bencode::to_bytes(self).expect("Failed to serialize info hash"));
        hasher.result(&mut hash);
        Ok(hash)
    }
}

#[derive(Debug, Fail, PartialEq)]
pub enum Error {
    #[fail(display = "invalid pieces array length: {}", _0)]
    InvalidPieceArrayLength(String),
    #[fail(display = "piece length equal to zero")]
    ZeroPieceLength,
    #[fail(display = "length equal to zero")]
    ZeroLength,
    #[fail(display = "invalid name")]
    InvalidName,
}

#[derive(Debug, Default, Deserialize)]
pub struct Metainfo {
    pub announce: String,
    pub info: Info,
    #[serde(rename = "creation date")]
    pub creation_date: Option<u64>,
    pub comment: Option<String>,
    pub created_by: Option<String>,
    pub encoding: Option<String>,
}

impl Metainfo {
    pub fn info_hash(&self) -> Result<[u8; 20], Error> {
        self.info.hash()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Read;

    #[test]
    fn test_validate_info() {
        let infos = vec![
            Info {
                name: "".to_owned(),
                piece_length: 1,
                pieces: vec![0; 40],
                length: 2,
            },
            Info {
                name: "test".to_owned(),
                piece_length: 1,
                pieces: vec![0; 1],
                length: 0,
            },
            Info {
                name: "test".to_owned(),
                piece_length: 0,
                pieces: vec![0; 1],
                length: 10,
            },
            Info {
                name: "test".to_owned(),
                piece_length: 100,
                pieces: vec![0; 20],
                length: 200,
            },
            Info {
                name: "test".to_owned(),
                piece_length: 100,
                pieces: vec![0; 20],
                length: 100,
            },
        ];

        assert_eq!(infos[0].validate().unwrap_err(), Error::InvalidName);
        assert_eq!(infos[1].validate().unwrap_err(), Error::ZeroLength);
        assert_eq!(infos[2].validate().unwrap_err(), Error::ZeroPieceLength);
        match infos[3].validate() {
            Err(Error::InvalidPieceArrayLength(_)) => (),
            _ => assert!(false),
        }
        assert!(infos[4].validate().is_ok());
    }
    #[test]
    fn test_info_hash() -> Result<(), failure::Error> {
        const TORRENT_FILE: &'static str = "data/test.torrent";
        let mut f = File::open(&TORRENT_FILE).expect(&format!("{} does ", &TORRENT_FILE));
        let mut b: Vec<u8> = Vec::new();
        f.read_to_end(&mut b)
            .expect(&format!("could not read {}", &TORRENT_FILE));
        let m: Metainfo = serde_bencode::from_bytes(&b).expect("Failed to deserialize");
        assert_eq!(
            m.info_hash()?,
            [
                231, 4, 155, 86, 57, 90, 222, 194, 139, 80, 224, 230, 244, 132, 159, 221, 49, 30,
                236, 117
            ]
        );
        Ok(())
    }
}
