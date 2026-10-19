use crate::json::{Array, Object, Value};
use alloc::collections::btree_map;
use alloc::string::String;
use alloc::vec::Vec;
use core::slice;

enum Frame<'a> {
    Array(slice::Iter<'a, Value>, Array),
    Object(btree_map::Iter<'a, String, Value>, Object, Option<String>),
}

pub fn deeply(value: &Value) -> Value {
    let mut stack = Vec::new();
    let mut done = begin(value, &mut stack);

    loop {
        let Some(frame) = stack.last_mut() else {
            return done.unwrap();
        };

        if let Some(value) = done.take() {
            match frame {
                Frame::Array(_, array) => array.push(value),
                Frame::Object(_, object, key) => {
                    object.insert(key.take().unwrap(), value);
                }
            }
        }

        let next = match frame {
            Frame::Array(iter, _) => iter.next(),
            Frame::Object(iter, _, key) => iter.next().map(|(k, v)| {
                *key = Some(k.clone());
                v
            }),
        };

        done = match next {
            Some(child) => begin(child, &mut stack),
            None => match stack.pop() {
                Some(Frame::Array(_, array)) => Some(Value::Array(array)),
                Some(Frame::Object(_, object, _)) => Some(Value::Object(object)),
                None => unreachable!(),
            },
        };
    }
}

fn begin<'a>(value: &'a Value, stack: &mut Vec<Frame<'a>>) -> Option<Value> {
    match value {
        Value::Null => Some(Value::Null),
        Value::Bool(boolean) => Some(Value::Bool(*boolean)),
        Value::Number(number) => Some(Value::Number(number.clone())),
        Value::String(string) => Some(Value::String(string.clone())),
        Value::Array(array) => {
            let mut clone = Array::new();
            clone.reserve_exact(array.len());
            stack.push(Frame::Array(array.iter(), clone));
            None
        }
        Value::Object(object) => {
            stack.push(Frame::Object(object.iter(), Object::new(), None));
            None
        }
    }
}
