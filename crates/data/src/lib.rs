pub mod attachment;
mod contact;
mod history;
pub mod index;
pub mod message;
mod state;
pub mod stickers;
mod thread;

pub use contact::{Contact, Group, Member, Role};
pub use history::History;
pub use index::{Index, Section};
pub use message::{
    Fragment, Message, MessageId, Reaction, Status, Wanted, classify, pointers, project,
    receipt_from_content,
};
pub use state::{Connection, State};
pub use thread::{ContactId, Thread};

/// Bytes as they are written down: a thread seed, a digest, a pack id. Five
/// copies of this were hand-rolled across four crates, three of them allocating
/// a `String` per byte -- and one of them is called once per sidebar row per
/// frame, on a group key of thirty-two, because the list is a scrolling `div`.
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: [u8; 16] = *b"0123456789abcdef";

    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 0xf)] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn bytes_are_written_two_digits_each() {
        assert_eq!(super::hex(&[0x00, 0x0f, 0xde, 0xad]), "000fdead");
        assert_eq!(super::hex(&[]), "");
    }
}
