use bitvec::BitVec;
use byteorder::{ReadBytesExt, WriteBytesExt, BE};
use failure::{self, Fail};
use std::io::{self, Read, Write};

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "invalid message id: {}", _0)]
    Invalid(u8),
    #[fail(
        display = "length less than min for type (min: {}, current: {})",
        _0, _1
    )]
    SmallLength(u32, u32),
    #[fail(
        display = "invalid received packet length (expected: {}, current: {})",
        _0, _1
    )]
    WrongLength(u32, u32),
    #[fail(display = "stream error: {}", _0)]
    IO(#[fail(cause)] io::Error),
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::IO(e)
    }
}

#[derive(Debug, PartialEq)]
pub enum Message {
    KeepAlive,
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have(u32),
    Request(u32, u32, u32), // piece index, byte index, length
    Cancel(u32, u32, u32),
    BitField(BitVec),
    Piece(u32, u32, Vec<u8>),
    Port(u16),
}

impl Message {
    fn len(&self) -> u32 {
        match self {
            Message::KeepAlive => 0,
            Message::Choke | Message::Unchoke | Message::Interested | Message::NotInterested => 1,
            Message::Port(_) => 3,
            Message::Have(_) => 5,
            Message::Request(_, _, _) | Message::Cancel(_, _, _) => 13,
            Message::BitField(ref bf) => bf.as_slice().len() as u32 + 1,
            Message::Piece(_, _, ref v) => 9 + v.len() as u32,
        }
    }

    fn id(&self) -> Option<u8> {
        match self {
            Message::KeepAlive => None,
            Message::Choke => Some(0),
            Message::Unchoke => Some(1),
            Message::Interested => Some(2),
            Message::NotInterested => Some(3),
            Message::Have(_) => Some(4),
            Message::BitField(_) => Some(5),
            Message::Request(_, _, _) => Some(6),
            Message::Piece(_, _, _) => Some(7),
            Message::Cancel(_, _, _) => Some(8),
            Message::Port(_) => Some(9),
        }
    }

    fn send_preamble<W: Write>(&self, mut writer: W) -> io::Result<()> {
        match self {
            Message::KeepAlive => writer.write_u32::<BE>(0),
            _ => {
                writer.write_u32::<BE>(self.len())?;
                writer.write_u8(self.id().unwrap())
            }
        }
    }

    fn validate(&self, expected_length: u32) -> Result<(), Error> {
        match self.len() {
            n if n == expected_length => Ok(()),
            _ => Err(Error::WrongLength(expected_length, self.len())),
        }
    }

    pub fn send<W: Write>(&self, mut writer: W) -> io::Result<()> {
        self.send_preamble(writer.by_ref())?;
        match *self {
            Message::KeepAlive
            | Message::Choke
            | Message::Unchoke
            | Message::Interested
            | Message::NotInterested => Ok(()),
            Message::Have(index) => writer.write_u32::<BE>(index),
            Message::Request(index, begin, length) | Message::Cancel(index, begin, length) => {
                writer.write_u32::<BE>(index)?;
                writer.write_u32::<BE>(begin)?;
                writer.write_u32::<BE>(length)
            }
            Message::BitField(ref bv) => writer.write_all(bv.as_slice()),
            Message::Piece(index, begin, ref v) => {
                writer.write_u32::<BE>(index)?;
                writer.write_u32::<BE>(begin)?;
                writer.write_all(v.as_slice())
            }
            Message::Port(port) => writer.write_u16::<BE>(port),
        }
    }

    /// If this function returns an error, then recovery is almost impossible, since no state about the channel is kept; it is undeterminable at what stage of message parsing an error occured.
    /// This means all non-terminal errors should be handled by this function.
    /// TODO: Possibly refactor this and message sending out into a Sender and Receiver Class
    pub fn receive<R: Read>(mut reader: R) -> Result<Self, Error> {
        let length = reader.read_u32::<BE>()?;
        if length == 0 {
            return Ok(Message::KeepAlive);
        }
        let id = reader.read_u8()?;
        let ret = match id {
            0 => Message::Choke,
            1 => Message::Unchoke,
            2 => Message::Interested,
            3 => Message::NotInterested,
            4 => Message::Have(reader.read_u32::<BE>()?),
            5 => {
                if length < 2 {
                    return Err(Error::SmallLength(2, length));
                }
                let bv_len = 4 * (length - 1);
                let mut bv = BitVec::with_capacity(bv_len as usize);
                unsafe { bv.set_len(bv_len as usize) }
                reader.read_exact(bv.as_mut_slice())?;
                Message::BitField(bv)
            }
            6 => {
                let index = reader.read_u32::<BE>()?;
                let begin = reader.read_u32::<BE>()?;
                let length = reader.read_u32::<BE>()?;
                Message::Request(index, begin, length)
            }
            7 => {
                let index = reader.read_u32::<BE>()?;
                let begin = reader.read_u32::<BE>()?;
                if length < 10 {
                    return Err(Error::SmallLength(10, length));
                }
                let vec_len = length - 9;
                let mut v: Vec<u8> = Vec::with_capacity(vec_len as usize);
                reader.take(vec_len.into()).read_to_end(&mut v)?;
                Message::Piece(index, begin, v)
            }
            8 => {
                let index = reader.read_u32::<BE>()?;
                let begin = reader.read_u32::<BE>()?;
                let length = reader.read_u32::<BE>()?;
                Message::Cancel(index, begin, length)
            }
            9 => Message::Port(reader.read_u16::<BE>()?),
            _ => return Err(Error::Invalid(id)),
        };
        ret.validate(length)?;
        Ok(ret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitvec::bitvec;
    use std::io::Cursor;

    #[test]
    fn test_send_message() -> Result<(), failure::Error> {
        let v = vec![
            Message::KeepAlive,
            Message::Choke,
            Message::Unchoke,
            Message::Interested,
            Message::NotInterested,
            Message::Have(2),
            Message::BitField(bitvec![0, 0, 0, 1]),
            Message::Request(2, 0, 3),
            Message::Piece(2, 0, vec![2, 8, 5]),
            Message::Cancel(2, 0, 3),
            Message::Port(1000),
        ];
        let mut it = v.iter();
        let mut d = Vec::with_capacity(13);

        // Keep Alive
        it.next().unwrap().send(&mut d)?;
        assert_eq!(d.as_slice(), &[0, 0, 0, 0]);
        d.clear();

        // Choke
        it.next().unwrap().send(&mut d)?;
        assert_eq!(d.as_slice(), &[0, 0, 0, 1, 0]);
        d.clear();

        // Unchoke
        it.next().unwrap().send(&mut d)?;
        assert_eq!(d.as_slice(), &[0, 0, 0, 1, 1]);
        d.clear();

        // Interested
        it.next().unwrap().send(&mut d)?;
        assert_eq!(d.as_slice(), &[0, 0, 0, 1, 2]);
        d.clear();

        // Not Interested
        it.next().unwrap().send(&mut d)?;
        assert_eq!(d.as_slice(), &[0, 0, 0, 1, 3]);
        d.clear();

        // Have
        it.next().unwrap().send(&mut d)?;
        assert_eq!(d.as_slice(), &[0, 0, 0, 5, 4, 0, 0, 0, 2]);
        d.clear();

        // Bitfield
        it.next().unwrap().send(&mut d)?;
        assert_eq!(d.as_slice(), &[0, 0, 0, 2, 5, 16]);
        d.clear();

        // Request
        it.next().unwrap().send(&mut d)?;
        assert_eq!(
            d.as_slice(),
            &[0, 0, 0, 13, 6, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 3]
        );
        d.clear();

        // Piece
        it.next().unwrap().send(&mut d)?;
        assert_eq!(
            d.as_slice(),
            &[0, 0, 0, 12, 7, 0, 0, 0, 2, 0, 0, 0, 0, 2, 8, 5]
        );
        d.clear();

        // Cancel
        it.next().unwrap().send(&mut d)?;
        assert_eq!(
            d.as_slice(),
            &[0, 0, 0, 13, 8, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 3]
        );
        d.clear();

        // Port
        it.next().unwrap().send(&mut d)?;
        assert_eq!(d.as_slice(), &[0, 0, 0, 3, 9, 3, 232]);
        d.clear();
        Ok(())
    }

    #[test]
    fn test_receive_message() -> Result<(), failure::Error> {
        let mut msg_buf: Cursor<&[u8]> = Cursor::new(&[
            0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 1, 0, 0, 0, 1, 2, 0, 0, 0, 1, 3, 0, 0, 0, 5, 4,
            0, 0, 0, 2, 0, 0, 0, 2, 5, 16, 0, 0, 0, 13, 6, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 3, 0,
            0, 0, 12, 7, 0, 0, 0, 2, 0, 0, 0, 0, 2, 8, 5, 0, 0, 0, 13, 8, 0, 0, 0, 2, 0, 0, 0, 0,
            0, 0, 0, 3, 0, 0, 0, 3, 9, 3, 232,
        ]);

        let v = vec![
            Message::KeepAlive,
            Message::Choke,
            Message::Unchoke,
            Message::Interested,
            Message::NotInterested,
            Message::Have(2),
            Message::BitField(bitvec![0, 0, 0, 1]),
            Message::Request(2, 0, 3),
            Message::Piece(2, 0, vec![2, 8, 5]),
            Message::Cancel(2, 0, 3),
            Message::Port(1000),
        ];
        let len = v.len();
        let mut it = v.into_iter();
        for _i in 0..len {
            assert_eq!(Message::receive(&mut msg_buf)?, it.next().unwrap());
        }
        Ok(())
    }
}
