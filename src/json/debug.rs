use crate::json::{Array, Object, Value};
use alloc::collections::btree_map;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::{self, Write};
use core::slice;

enum Frame<'a> {
    Array(slice::Iter<'a, Value>, bool),
    Object(btree_map::Iter<'a, String, Value>, bool),
}

pub fn array(array: &Array, formatter: &mut fmt::Formatter) -> fmt::Result {
    let mut stack = Vec::new();
    formatter.write_str("Array [")?;
    stack.push(Frame::Array(array.iter(), true));
    drain(stack, formatter)
}

pub fn object(object: &Object, formatter: &mut fmt::Formatter) -> fmt::Result {
    let mut stack = Vec::new();
    formatter.write_str("Object {")?;
    stack.push(Frame::Object(object.iter(), true));
    drain(stack, formatter)
}

pub fn value(value: &Value, formatter: &mut fmt::Formatter) -> fmt::Result {
    let mut stack = Vec::new();
    begin(value, &mut stack, formatter)?;
    drain(stack, formatter)
}

fn begin<'a>(
    value: &'a Value,
    stack: &mut Vec<Frame<'a>>,
    formatter: &mut fmt::Formatter,
) -> fmt::Result {
    match value {
        Value::Null => formatter.write_str("Null"),
        Value::Bool(boolean) => write!(formatter, "Bool({})", boolean),
        Value::Number(number) => write!(formatter, "Number({})", number),
        Value::String(string) => write!(formatter, "String({:?})", string),
        Value::Array(array) => {
            stack.push(Frame::Array(array.iter(), true));
            formatter.write_str("Array [")
        }
        Value::Object(object) => {
            stack.push(Frame::Object(object.iter(), true));
            formatter.write_str("Object {")
        }
    }
}

fn drain<'a>(mut stack: Vec<Frame<'a>>, formatter: &mut fmt::Formatter) -> fmt::Result {
    let pretty = formatter.alternate();
    loop {
        let depth = stack.len();
        let Some(frame) = stack.last_mut() else {
            return Ok(());
        };
        let (next, first) = match frame {
            Frame::Array(iter, first) => (iter.next().map(|value| (None, value)), first),
            Frame::Object(iter, first) => (iter.next().map(|(k, v)| (Some(k), v)), first),
        };
        let was_first = *first;
        *first = false;

        if pretty && !was_first {
            formatter.write_str(",\n")?;
        }

        let Some((key, value)) = next else {
            if pretty && !was_first {
                indent(formatter, depth - 1)?;
            }
            match stack.pop() {
                Some(Frame::Array(..)) => formatter.write_char(']')?,
                Some(Frame::Object(..)) => formatter.write_char('}')?,
                None => unreachable!(),
            }
            continue;
        };

        if pretty {
            if was_first {
                formatter.write_char('\n')?;
            }
            indent(formatter, depth)?;
        } else if !was_first {
            formatter.write_str(", ")?;
        }
        if let Some(key) = key {
            write!(formatter, "{:?}: ", key)?;
        }
        begin(value, &mut stack, formatter)?;
    }
}

fn indent(formatter: &mut fmt::Formatter, depth: usize) -> fmt::Result {
    for _ in 0..depth {
        formatter.write_str("    ")?;
    }
    Ok(())
}
