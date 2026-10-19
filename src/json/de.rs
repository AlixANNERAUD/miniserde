use self::Event::*;
use crate::de::{Deserialize, Map, Seq, Visitor};
use crate::error::{Error, Result};
use crate::ptr::NonuniqueBox;
use alloc::vec::Vec;
use core::char;
use core::ptr::NonNull;
use core::str;

/// Deserialize a JSON string into any deserializable type.
///
/// ```rust
/// use miniserde::{json, Deserialize};
///
/// #[derive(Deserialize, Debug)]
/// struct Example {
///     code: u32,
///     message: String,
/// }
///
/// fn main() -> miniserde::Result<()> {
///     let j = r#" {"code": 200, "message": "reminiscent of Serde"} "#;
///
///     let out: Example = json::from_str(&j)?;
///     println!("{:?}", out);
///
///     Ok(())
/// }
/// ```
pub fn from_str<T>(j: &str) -> Result<T>
where
    T: Deserialize,
{
    let mut out = None;
    from_str_impl(j, T::begin(&mut out))?;
    out.ok_or(Error)
}

struct Deserializer<'a, 'b> {
    input: &'a [u8],
    pos: usize,
    number_start: usize,
    buffer: Vec<u8>,
    stack: Vec<(NonNull<dyn Visitor>, Layer<'b>)>,
}

enum Layer<'a> {
    Seq(NonuniqueBox<dyn Seq + 'a>),
    Map(NonuniqueBox<dyn Map + 'a>),
}

impl<'a, 'b> Drop for Deserializer<'a, 'b> {
    fn drop(&mut self) {
        // Drop layers in reverse order.
        while !self.stack.is_empty() {
            self.stack.pop();
        }
    }
}

fn from_str_impl(j: &str, visitor: &mut dyn Visitor) -> Result<()> {
    let visitor = NonNull::from(visitor);
    let mut visitor = unsafe { extend_lifetime!(visitor as NonNull<dyn Visitor>) };
    let mut de = Deserializer {
        input: j.as_bytes(),
        pos: 0,
        number_start: 0,
        buffer: Vec::new(),
        stack: Vec::new(),
    };

    'outer: loop {
        let visitor_mut = unsafe { &mut *visitor.as_ptr() };
        let layer = match de.event()? {
            Null => {
                visitor_mut.null()?;
                None
            }
            Bool(b) => {
                visitor_mut.boolean(b)?;
                None
            }
            Negative(n) => {
                visitor_mut.negative(n)?;
                None
            }
            Nonnegative(n) => {
                visitor_mut.nonnegative(n)?;
                None
            }
            Float(n) => {
                visitor_mut.float(n)?;
                None
            }
            Str(s) => {
                visitor_mut.string(s)?;
                None
            }
            SeqStart => {
                let seq = visitor_mut.seq()?;
                Some(Layer::Seq(NonuniqueBox::from(seq)))
            }
            MapStart => {
                let map = visitor_mut.map()?;
                Some(Layer::Map(NonuniqueBox::from(map)))
            }
        };

        let mut accept_comma;
        let mut layer = match layer {
            Some(layer) => {
                accept_comma = false;
                layer
            }
            None => match de.stack.pop() {
                Some(frame) => {
                    accept_comma = true;
                    visitor = frame.0;
                    frame.1
                }
                None => break 'outer,
            },
        };

        loop {
            match de.parse_whitespace().unwrap_or(b'\0') {
                b',' if accept_comma => {
                    de.bump();
                    break;
                }
                close @ (b']' | b'}') => {
                    de.bump();
                    match &mut layer {
                        Layer::Seq(seq) if close == b']' => seq.finish()?,
                        Layer::Map(map) if close == b'}' => map.finish()?,
                        _ => return Err(Error),
                    }
                    let Some(frame) = de.stack.pop() else {
                        break 'outer;
                    };
                    accept_comma = true;
                    visitor = frame.0;
                    layer = frame.1;
                }
                _ => {
                    if accept_comma {
                        return Err(Error);
                    } else {
                        break;
                    }
                }
            }
        }

        let outer = visitor;
        match layer {
            Layer::Seq(mut seq) => {
                let element = seq.element()?;
                let next = NonNull::from(element);
                visitor = unsafe { extend_lifetime!(next as NonNull<dyn Visitor>) };
                de.stack.push((outer, Layer::Seq(seq)));
            }
            Layer::Map(mut map) => {
                match de.parse_whitespace() {
                    Some(b'"') => de.bump(),
                    _ => return Err(Error),
                }
                let key = de.parse_str()?;
                let entry = map.key(key)?;
                let next = NonNull::from(entry);
                visitor = unsafe { extend_lifetime!(next as NonNull<dyn Visitor>) };
                match de.parse_whitespace() {
                    Some(b':') => de.bump(),
                    _ => return Err(Error),
                }
                de.stack.push((outer, Layer::Map(map)));
            }
        }
    }

    match de.parse_whitespace() {
        Some(_) => Err(Error),
        None => Ok(()),
    }
}

enum Event<'a> {
    Null,
    Bool(bool),
    Str(&'a str),
    Negative(i64),
    Nonnegative(u64),
    Float(f64),
    SeqStart,
    MapStart,
}

macro_rules! overflow {
    ($a:ident * 10 + $b:ident, $c:expr) => {
        match $c {
            c => $a >= c / 10 && ($a > c / 10 || $b > c % 10),
        }
    };
}

impl<'a, 'b> Deserializer<'a, 'b> {
    fn next(&mut self) -> Option<u8> {
        if self.pos < self.input.len() {
            let ch = self.input[self.pos];
            self.pos += 1;
            Some(ch)
        } else {
            None
        }
    }

    fn next_or_nul(&mut self) -> u8 {
        self.next().unwrap_or(b'\0')
    }

    fn peek(&mut self) -> Option<u8> {
        if self.pos < self.input.len() {
            Some(self.input[self.pos])
        } else {
            None
        }
    }

    fn peek_or_nul(&mut self) -> u8 {
        self.peek().unwrap_or(b'\0')
    }

    fn bump(&mut self) {
        self.pos += 1;
    }

    fn parse_str(&mut self) -> Result<&str> {
        fn result(bytes: &[u8]) -> &str {
            // The deserialization input came in as &str with a UTF-8 guarantee,
            // and the \u-escapes are checked along the way, so don't need to
            // check here.
            unsafe { str::from_utf8_unchecked(bytes) }
        }

        // Index of the first byte not yet copied into the scratch space.
        let mut start = self.pos;
        self.buffer.clear();

        loop {
            while self.pos < self.input.len() && !ESCAPE[usize::from(self.input[self.pos])] {
                self.pos += 1;
            }
            if self.pos == self.input.len() {
                return Err(Error);
            }
            match self.input[self.pos] {
                b'"' => {
                    if self.buffer.is_empty() {
                        // Fast path: return a slice of the raw JSON without any
                        // copying.
                        let borrowed = &self.input[start..self.pos];
                        self.pos += 1;
                        return Ok(result(borrowed));
                    } else {
                        self.buffer.extend_from_slice(&self.input[start..self.pos]);
                        self.pos += 1;
                        return Ok(result(&self.buffer));
                    }
                }
                b'\\' => {
                    self.buffer.extend_from_slice(&self.input[start..self.pos]);
                    self.pos += 1;
                    self.parse_escape()?;
                    start = self.pos;
                }
                _ => {
                    return Err(Error);
                }
            }
        }
    }

    fn next_or_eof(&mut self) -> Result<u8> {
        self.next().ok_or(Error)
    }

    /// Parses a JSON escape sequence and appends it into the scratch space. Assumes
    /// the previous byte read was a backslash.
    fn parse_escape(&mut self) -> Result<()> {
        let ch = self.next_or_eof()?;

        match ch {
            b'"' => self.buffer.push(b'"'),
            b'\\' => self.buffer.push(b'\\'),
            b'/' => self.buffer.push(b'/'),
            b'b' => self.buffer.push(b'\x08'),
            b'f' => self.buffer.push(b'\x0c'),
            b'n' => self.buffer.push(b'\n'),
            b'r' => self.buffer.push(b'\r'),
            b't' => self.buffer.push(b'\t'),
            b'u' => {
                let c = match self.decode_hex_escape()? {
                    0xDC00..=0xDFFF => {
                        return Err(Error);
                    }

                    // Non-BMP characters are encoded as a sequence of
                    // two hex escapes, representing UTF-16 surrogates.
                    n1 @ 0xD800..=0xDBFF => {
                        if self.next_or_eof()? != b'\\' {
                            return Err(Error);
                        }
                        if self.next_or_eof()? != b'u' {
                            return Err(Error);
                        }

                        let n2 = self.decode_hex_escape()?;

                        if n2 < 0xDC00 || n2 > 0xDFFF {
                            return Err(Error);
                        }

                        let n =
                            ((u32::from(n1 - 0xD800) << 10) | u32::from(n2 - 0xDC00)) + 0x1_0000;

                        match char::from_u32(n) {
                            Some(c) => c,
                            None => {
                                return Err(Error);
                            }
                        }
                    }

                    n => match char::from_u32(u32::from(n)) {
                        Some(c) => c,
                        None => {
                            return Err(Error);
                        }
                    },
                };

                self.buffer
                    .extend_from_slice(c.encode_utf8(&mut [0_u8; 4]).as_bytes());
            }
            _ => {
                return Err(Error);
            }
        }

        Ok(())
    }

    fn decode_hex_escape(&mut self) -> Result<u16> {
        let mut n = 0;
        for _ in 0..4 {
            n = match self.next_or_eof()? {
                c @ b'0'..=b'9' => n * 16_u16 + u16::from(c - b'0'),
                b'a' | b'A' => n * 16_u16 + 10_u16,
                b'b' | b'B' => n * 16_u16 + 11_u16,
                b'c' | b'C' => n * 16_u16 + 12_u16,
                b'd' | b'D' => n * 16_u16 + 13_u16,
                b'e' | b'E' => n * 16_u16 + 14_u16,
                b'f' | b'F' => n * 16_u16 + 15_u16,
                _ => {
                    return Err(Error);
                }
            };
        }
        Ok(n)
    }

    fn parse_whitespace(&mut self) -> Option<u8> {
        loop {
            match self.peek() {
                Some(b' ' | b'\n' | b'\t' | b'\r') => {
                    self.bump();
                }
                other => {
                    return other;
                }
            }
        }
    }

    fn parse_ident(&mut self, ident: &[u8]) -> Result<()> {
        for expected in ident {
            match self.next() {
                None => {
                    return Err(Error);
                }
                Some(next) => {
                    if next != *expected {
                        return Err(Error);
                    }
                }
            }
        }
        Ok(())
    }

    fn parse_integer(&mut self, nonnegative: bool, first_digit: u8) -> Result<Event> {
        match first_digit {
            b'0' => {
                // There can be only one leading '0'.
                match self.peek_or_nul() {
                    b'0'..=b'9' => Err(Error),
                    _ => self.parse_number(nonnegative, 0),
                }
            }
            c @ b'1'..=b'9' => {
                let mut res = u64::from(c - b'0');

                loop {
                    match self.peek_or_nul() {
                        c @ b'0'..=b'9' => {
                            self.bump();
                            let digit = u64::from(c - b'0');

                            // We need to be careful with overflow. If we can, try to keep the
                            // number as a `u64` until we grow too large. At that point, switch to
                            // parsing the value as a `f64`.
                            if overflow!(res * 10 + digit, u64::MAX) {
                                return self.parse_long_integer().map(Float);
                            }

                            res = res * 10 + digit;
                        }
                        _ => {
                            return self.parse_number(nonnegative, res);
                        }
                    }
                }
            }
            _ => Err(Error),
        }
    }

    fn parse_long_integer(&mut self) -> Result<f64> {
        while let b'0'..=b'9' = self.peek_or_nul() {
            self.bump();
        }

        match self.peek_or_nul() {
            b'.' => self.parse_decimal(),
            b'e' | b'E' => self.parse_exponent(),
            _ => self.f64_from_input(),
        }
    }

    fn parse_number(&mut self, nonnegative: bool, significand: u64) -> Result<Event> {
        match self.peek_or_nul() {
            b'.' => self.parse_decimal().map(Float),
            b'e' | b'E' => self.parse_exponent().map(Float),
            _ => {
                Ok(if nonnegative {
                    Nonnegative(significand)
                } else {
                    let neg = (significand as i64).wrapping_neg();

                    // Convert into a float if we underflow.
                    if neg > 0 {
                        Float(-(significand as f64))
                    } else {
                        Negative(neg)
                    }
                })
            }
        }
    }

    fn parse_decimal(&mut self) -> Result<f64> {
        self.bump();

        let mut at_least_one_digit = false;
        while let b'0'..=b'9' = self.peek_or_nul() {
            self.bump();
            at_least_one_digit = true;
        }

        if !at_least_one_digit {
            return Err(Error);
        }

        match self.peek_or_nul() {
            b'e' | b'E' => self.parse_exponent(),
            _ => self.f64_from_input(),
        }
    }

    fn parse_exponent(&mut self) -> Result<f64> {
        self.bump();

        if let b'+' | b'-' = self.peek_or_nul() {
            self.bump();
        }

        // Make sure a digit follows the exponent place.
        let (b'0'..=b'9') = self.next_or_nul() else {
            return Err(Error);
        };

        while let b'0'..=b'9' = self.peek_or_nul() {
            self.bump();
        }

        self.f64_from_input()
    }

    fn f64_from_input(&self) -> Result<f64> {
        let number = str::from_utf8(&self.input[self.number_start..self.pos]).map_err(|_| Error)?;
        match number.parse::<f64>() {
            Ok(f) if f.is_finite() => Ok(f),
            _ => Err(Error),
        }
    }

    fn event(&mut self) -> Result<Event> {
        let Some(peek) = self.parse_whitespace() else {
            return Err(Error);
        };
        self.number_start = self.pos;
        self.bump();
        match peek {
            b'"' => self.parse_str().map(Str),
            digit @ b'0'..=b'9' => self.parse_integer(true, digit),
            b'-' => {
                let first_digit = self.next_or_nul();
                self.parse_integer(false, first_digit)
            }
            b'{' => Ok(MapStart),
            b'[' => Ok(SeqStart),
            b'n' => {
                self.parse_ident(b"ull")?;
                Ok(Null)
            }
            b't' => {
                self.parse_ident(b"rue")?;
                Ok(Bool(true))
            }
            b'f' => {
                self.parse_ident(b"alse")?;
                Ok(Bool(false))
            }
            _ => Err(Error),
        }
    }
}

const CT: bool = true; // control character \x00..=\x1F
const QU: bool = true; // quote \x22
const BS: bool = true; // backslash \x5C
const O: bool = false; // allow unescaped

// Lookup table of bytes that must be escaped. A value of true at index i means
// that byte i requires an escape sequence in the input.
#[rustfmt::skip]
static ESCAPE: [bool; 256] = [
    //   1   2   3   4   5   6   7   8   9   A   B   C   D   E   F
    CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, // 0
    CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, // 1
     O,  O, QU,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // 2
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // 3
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // 4
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, BS,  O,  O,  O, // 5
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // 6
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // 7
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // 8
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // 9
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // A
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // B
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // C
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // D
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // E
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // F
];
