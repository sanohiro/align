//! Structural JSON validation for the checked encoder's symbolic output stream.
//! Dynamic pieces denote complete values; optional fields share a trailing-comma protocol.
use crate::TemplatePiece;
use std::collections::HashSet;

#[derive(Clone, Copy)]
enum Event<'a> {
    Byte(u8),
    Piece(&'a TemplatePiece),
}

struct Stream<'a> {
    pieces: &'a [TemplatePiece],
    index: usize,
    offset: usize,
    // None means an optional field may have emitted either nothing or a comma.
    last: Option<u8>,
}

impl<'a> Stream<'a> {
    fn peek(&mut self) -> Option<Event<'a>> {
        loop {
            match self.pieces.get(self.index)? {
                TemplatePiece::Static(text) => {
                    if let Some(&byte) = text.as_bytes().get(self.offset) {
                        return Some(Event::Byte(byte));
                    }
                    self.index += 1;
                    self.offset = 0;
                }
                piece => return Some(Event::Piece(piece)),
            }
        }
    }
    fn byte(&mut self) -> Option<u8> {
        let Event::Byte(byte) = self.peek()? else {
            return None;
        };
        self.offset += 1;
        self.last = Some(byte);
        Some(byte)
    }
    fn take(&mut self, expected: u8) -> bool {
        self.byte() == Some(expected)
    }
    fn piece(&mut self) {
        self.index += 1;
        self.offset = 0;
    }
    fn ws(&mut self) {
        while matches!(self.peek(), Some(Event::Byte(b' ' | b'\t' | b'\r' | b'\n'))) {
            self.byte();
        }
    }
    fn hex4(&mut self) -> Option<u32> {
        let mut value = 0;
        for _ in 0..4 {
            value = value * 16 + char::from(self.byte()?).to_digit(16)?;
        }
        Some(value)
    }
    fn string(&mut self) -> Option<Vec<u8>> {
        if !self.take(b'"') {
            return None;
        }
        let mut text = Vec::new();
        loop {
            match self.byte()? {
                b'"' => return Some(text),
                0..=31 => return None,
                b'\\' => match self.byte()? {
                    c @ (b'"' | b'\\' | b'/') => text.push(c),
                    b'b' => text.push(8),
                    b'f' => text.push(12),
                    b'n' => text.push(10),
                    b'r' => text.push(13),
                    b't' => text.push(9),
                    b'u' => {
                        let first = self.hex4()?;
                        let scalar = if (0xd800..=0xdbff).contains(&first) {
                            if !self.take(b'\\') || !self.take(b'u') {
                                return None;
                            }
                            let second = self.hex4()?;
                            if !(0xdc00..=0xdfff).contains(&second) {
                                return None;
                            }
                            0x10000 + ((first - 0xd800) << 10) + second - 0xdc00
                        } else {
                            first
                        };
                        let scalar = char::from_u32(scalar)?;
                        let mut bytes = [0; 4];
                        text.extend_from_slice(scalar.encode_utf8(&mut bytes).as_bytes());
                    }
                    _ => return None,
                },
                c => text.push(c),
            }
        }
    }
    fn number(&mut self) -> bool {
        if matches!(self.peek(), Some(Event::Byte(b'-'))) {
            self.byte();
        }
        match self.byte() {
            Some(b'0') => {
                if matches!(self.peek(), Some(Event::Byte(b'0'..=b'9'))) {
                    return false;
                }
            }
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return false,
        }
        if matches!(self.peek(), Some(Event::Byte(b'.'))) {
            self.byte();
            if !self.digits() {
                return false;
            }
        }
        if matches!(self.peek(), Some(Event::Byte(b'e' | b'E'))) {
            self.byte();
            if matches!(self.peek(), Some(Event::Byte(b'+' | b'-'))) {
                self.byte();
            }
            if !self.digits() {
                return false;
            }
        }
        true
    }
    fn digits(&mut self) -> bool {
        let mut any = false;
        while matches!(self.peek(), Some(Event::Byte(b'0'..=b'9'))) {
            self.byte();
            any = true;
        }
        any
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    First,
    Member,
    Colon,
    Value,
    After,
    Optional,
    Pop,
}
enum Frame {
    Root(bool),
    Array(Phase),
    Object {
        phase: Phase,
        optional: bool,
        keys: HashSet<Vec<u8>>,
    },
}

fn identifier(name: &[u8]) -> bool {
    !name.is_empty()
        && name
            .iter()
            .enumerate()
            .all(|(i, c)| c.is_ascii_alphabetic() || *c == b'_' || (i > 0 && c.is_ascii_digit()))
}

/// Validate structure without inferring the absent source schema or evaluating operands.
/// Operand widths and descriptor graphs are independently checked by the emitter's type gate.
pub fn json_encode_sequence_is_valid(pieces: &[TemplatePiece]) -> bool {
    fn validate(pieces: &[TemplatePiece]) -> Option<()> {
        let mut stream = Stream {
            pieces,
            index: 0,
            offset: 0,
            last: Some(0),
        };
        let mut stack = vec![Frame::Root(false)];
        loop {
            let frame = stack.last_mut()?;
            if !matches!(
                frame,
                Frame::Object {
                    phase: Phase::Pop,
                    ..
                }
            ) {
                stream.ws();
            }
            match frame {
                Frame::Root(true) => return stream.peek().is_none().then_some(()),
                Frame::Root(done) => {
                    *done = true;
                }
                Frame::Array(phase) => match *phase {
                    Phase::First if matches!(stream.peek(), Some(Event::Byte(b']'))) => {
                        stream.byte();
                        stack.pop();
                        continue;
                    }
                    Phase::First | Phase::Value => {
                        *phase = Phase::After;
                    }
                    Phase::After => match stream.byte()? {
                        b',' => {
                            *phase = Phase::Value;
                            continue;
                        }
                        b']' => {
                            stack.pop();
                            continue;
                        }
                        _ => return None,
                    },
                    _ => return None,
                },
                Frame::Object {
                    phase,
                    optional,
                    keys,
                } => match *phase {
                    Phase::First | Phase::Member | Phase::Optional => match stream.peek()? {
                        Event::Byte(b'}') if *phase == Phase::First && !*optional => {
                            stream.byte();
                            stack.pop();
                            continue;
                        }
                        Event::Piece(TemplatePiece::PopComma) => {
                            if !*optional || !matches!(stream.last, None | Some(b'{' | b',')) {
                                return None;
                            }
                            stream.piece();
                            *phase = Phase::Pop;
                            continue;
                        }
                        Event::Piece(
                            TemplatePiece::OptionField { name, .. }
                            | TemplatePiece::OptionStructField { name, .. },
                        ) => {
                            if !identifier(name.as_bytes())
                                || !keys.insert(name.as_bytes().to_vec())
                                || !matches!(stream.last, None | Some(b'{' | b','))
                            {
                                return None;
                            }
                            stream.piece();
                            stream.last = None;
                            *optional = true;
                            *phase = Phase::Optional;
                            continue;
                        }
                        Event::Byte(b'"') => {
                            let key = stream.string()?;
                            if !identifier(&key) || !keys.insert(key) {
                                return None;
                            }
                            *phase = Phase::Colon;
                            continue;
                        }
                        _ => return None,
                    },
                    Phase::Colon => {
                        if !stream.take(b':') {
                            return None;
                        }
                        *phase = Phase::Value;
                        continue;
                    }
                    Phase::Value => {
                        *phase = Phase::After;
                    }
                    Phase::After => match stream.byte()? {
                        b',' => {
                            *phase = Phase::Member;
                            continue;
                        }
                        b'}' if !*optional => {
                            stack.pop();
                            continue;
                        }
                        _ => return None,
                    },
                    Phase::Pop => {
                        if !stream.take(b'}') {
                            return None;
                        }
                        stack.pop();
                        continue;
                    }
                },
            }
            match stream.peek()? {
                Event::Byte(b'{') => {
                    stream.byte();
                    stack.push(Frame::Object {
                        phase: Phase::First,
                        optional: false,
                        keys: HashSet::new(),
                    });
                }
                Event::Byte(b'[') => {
                    stream.byte();
                    stack.push(Frame::Array(Phase::First));
                }
                Event::Byte(b'"') => {
                    stream.string()?;
                }
                Event::Byte(b'-' | b'0'..=b'9') => {
                    if !stream.number() {
                        return None;
                    }
                }
                Event::Byte(c @ (b't' | b'f' | b'n')) => {
                    let word: &[u8] = match c {
                        b't' => b"true",
                        b'f' => b"false",
                        _ => b"null",
                    };
                    for &byte in word {
                        if !stream.take(byte) {
                            return None;
                        }
                    }
                }
                Event::Piece(
                    TemplatePiece::IntHole(_)
                    | TemplatePiece::FloatHole(_)
                    | TemplatePiece::BoolHole(_)
                    | TemplatePiece::JsonStrHole(_)
                    | TemplatePiece::OwnedJsonRecords { .. }
                    | TemplatePiece::StructArrayField { .. }
                    | TemplatePiece::ScalarArrayField { .. }
                    | TemplatePiece::UnionValue { .. },
                ) => {
                    stream.piece();
                    stream.last = Some(0);
                }
                _ => return None,
            }
        }
    }
    validate(pieces).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Const, Operand};
    fn text(s: &str) -> TemplatePiece {
        TemplatePiece::Static(s.to_string())
    }
    fn hole() -> TemplatePiece {
        TemplatePiece::BoolHole(Operand::Const(Const::Bool(true)))
    }
    fn opt(name: &str) -> TemplatePiece {
        TemplatePiece::OptionField {
            name: name.to_string(),
            opt: Operand::Arg(0),
        }
    }

    #[test]
    fn r63_sequence_delimiters_tokens_and_optional_protocol() {
        for valid in [
            vec![text("{\"x\":"), hole(), text("}")],
            vec![text("{\""), text("x"), text("\":"), hole(), text("}")],
            vec![
                text("{"),
                opt("a"),
                opt("b"),
                TemplatePiece::PopComma,
                text("}"),
            ],
            vec![
                text("[{"),
                opt("a"),
                text("\"b\":"),
                hole(),
                text(","),
                TemplatePiece::PopComma,
                text("}]"),
            ],
            vec![text("{\"a\":[1,-0.3e+2,true,null,\"x\\u0000\"]}")],
        ] {
            assert!(json_encode_sequence_is_valid(&valid), "{valid:?}");
        }
        for invalid in [
            vec![text("{\"x\":"), hole(), text("]")],
            vec![text("{\"x\":"), hole(), text(",}")],
            vec![text("{"), opt("a"), text("}")],
            vec![
                text("{"),
                opt("a"),
                opt("a"),
                TemplatePiece::PopComma,
                text("}"),
            ],
            vec![text("{"), opt("a\""), TemplatePiece::PopComma, text("}")],
            vec![
                text("{"),
                opt("a"),
                TemplatePiece::PopComma,
                TemplatePiece::PopComma,
                text("}"),
            ],
            vec![
                text("{"),
                opt("a"),
                text(" "),
                TemplatePiece::PopComma,
                text("}"),
            ],
            vec![text("["), opt("a"), TemplatePiece::PopComma, text("]")],
            vec![text("["), hole(), TemplatePiece::PopComma, text("]")],
            vec![text("{\"x"), hole(), text("\":0}")],
            vec![text("1"), hole()],
            vec![text("{\"a\":01}")],
            vec![text("{\"a\":1e}")],
            vec![text("{\"a\":NaN}")],
            vec![text("{\"a\":truefalse}")],
            vec![text("{\"a\":\"\\uD800\"}")],
            vec![text("{\"a\":null,\"a\":null}")],
        ] {
            assert!(!json_encode_sequence_is_valid(&invalid), "{invalid:?}");
        }
    }
}
