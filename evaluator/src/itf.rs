//! Conversion between [`Value`]s and ITF (Informal Trace Format).
//!
//! Read more about it [here](https://apalache-mc.org/docs/adr/015adr-trace.html).
//!
//! This format can be parsed by Quint's typescript tool and by the ITF trace
//! viewer extension on VSCode.

use crate::trace_quality::TraceQuality;
use crate::value::{Str, Value, ValueRef};
use chrono;
use itf;
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct Trace {
    pub states: Vec<State>,
    pub violation: bool,
    pub seed: u64,
}

#[derive(Debug)]
pub struct State {
    pub value: Value,
    pub diagnostics: Vec<DebugMessage>,
}

#[derive(Debug)]
pub struct DebugMessage {
    pub label: Str,
    pub value: Value,
}

impl serde::Serialize for DebugMessage {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = s.serialize_struct("DebugMessage", 2)?;
        s.serialize_field("label", self.label.as_str())?;
        s.serialize_field("value", &self.value.to_itf())?;
        s.end()
    }
}

impl TraceQuality for Trace {
    fn is_violation(&self) -> bool {
        self.violation
    }

    fn has_diagnostics(&self) -> bool {
        self.states.iter().any(|s| !s.diagnostics.is_empty())
    }

    fn trace_length(&self) -> usize {
        self.states.len()
    }
}

impl Trace {
    pub fn to_itf(self, source: String) -> itf::Trace<itf::Value> {
        self.to_itf_with_pending_diagnostics(source, vec![])
    }

    pub fn to_itf_with_pending_diagnostics(
        self,
        source: String,
        pending_diagnostics: Vec<DebugMessage>,
    ) -> itf::Trace<itf::Value> {
        let mut vars = Vec::new();
        if let Some(state) = self.states.first() {
            // Find the variable names by taking the fields from the first
            // state (which should be a record).
            let ValueRef::Record(rec) = state.value.view() else {
                panic!("Expected a record, got {}", state.value);
            };
            vars.extend(rec.keys().map(|key| key.to_string()));
        }

        let mut states = Vec::new();
        for (state, i) in self.states.into_iter().zip(0u64..) {
            let mut other = BTreeMap::new();
            other.insert(
                "diagnostics".to_string(),
                serde_json::to_string(&state.diagnostics).expect("failed to serialize diagnostics"),
            );

            states.push(itf::State {
                meta: itf::state::Meta {
                    index: Some(i),
                    other,
                },
                value: state.value.to_itf(),
            });
        }

        let mut other = BTreeMap::new();
        other.insert(
            "status".to_string(),
            if self.violation {
                "violation".to_string()
            } else {
                "ok".to_string()
            },
        );
        if !pending_diagnostics.is_empty() {
            other.insert(
                "pending_diagnostics".to_string(),
                serde_json::to_string(&pending_diagnostics)
                    .expect("failed to serialize pending diagnostics"),
            );
        }

        itf::Trace {
            meta: itf::trace::Meta {
                format: Some("ITF".to_string()),
                format_description: Some(
                    "https://apalache-mc.org/docs/adr/015adr-trace.html".to_string(),
                ),
                source: Some(source),
                description: Some(format!(
                    "Created by Quint (Rust version) on {}",
                    chrono::offset::Local::now().to_rfc2822()
                )),
                var_types: BTreeMap::default(),
                timestamp: Some(chrono::offset::Local::now().timestamp_millis() as u64),
                other,
            },
            vars,
            states,
            params: vec![],
            loop_index: None,
        }
    }
}

impl Value {
    /// Convert an ITF value directly to a Quint [`Value`].
    pub fn from_itf(itf_value: itf::Value) -> Self {
        match itf_value {
            itf::Value::Bool(b) => Value::bool(b),
            itf::Value::Number(n) => Value::int(n),
            itf::Value::String(s) => Value::str(Str::from(s)),
            itf::Value::BigInt(b) => Value::bigint(b.into_inner()),
            itf::Value::List(elems) => {
                Value::list(elems.into_iter().map(Value::from_itf).collect())
            }
            itf::Value::Tuple(tup) => Value::tuple(tup.into_iter().map(Value::from_itf).collect()),
            itf::Value::Set(set) => Value::set(set.into_iter().map(Value::from_itf).collect()),
            itf::Value::Map(map) => Value::map(
                map.into_iter()
                    .map(|(k, v)| (Value::from_itf(k), Value::from_itf(v)))
                    .collect(),
            ),
            itf::Value::Record(rec) => {
                // Variants are encoded as records with "tag" and "value" fields.
                // Detect that pattern and convert back to a Variant.
                if rec.len() == 2 {
                    if let (Some(itf::Value::String(tag)), Some(value)) =
                        (rec.get("tag"), rec.get("value"))
                    {
                        return Value::variant(
                            Str::from(tag.clone()),
                            Value::from_itf(value.clone()),
                        );
                    }
                }
                // Apalache encodes empty tuples as { tag: "UNIT" }
                if rec.len() == 1 {
                    if let Some(itf::Value::String(tag)) = rec.get("tag") {
                        if tag == "UNIT" {
                            return Value::tuple(Default::default());
                        }
                    }
                }
                Value::record(
                    rec.into_iter()
                        .map(|(k, v)| (Str::from(k), Value::from_itf(v)))
                        .collect(),
                )
            }
            itf::Value::Unserializable(_) => {
                panic!("Cannot convert unserializable ITF value to Value")
            }
        }
    }

    pub fn to_itf(&self) -> itf::Value {
        match self.view() {
            // json-bigint parses a JSON number as a bignumber.js value when the
            // token is longer than 15 characters. The minus sign counts, so the
            // safe range is (-10^14, 10^15): -10^14 is 16 characters. Longer
            // integers are written as `#bigint`, the standard ITF encoding.
            ValueRef::Int(i) if json_number_text_len(i) <= 15 => itf::Value::Number(i),
            ValueRef::Int(i) => itf::Value::BigInt(itf::value::BigInt::new(i)),
            ValueRef::BigInt(n) => itf::Value::BigInt(itf::value::BigInt::new(n.clone())),
            ValueRef::Bool(b) => itf::Value::Bool(b),
            ValueRef::Str(s) => itf::Value::String(s.to_string()),
            ValueRef::InfiniteInt => itf::Value::Unserializable(
                serde_json::from_value(serde_json::json!({"#unserializable": "Int"})).unwrap(),
            ),
            ValueRef::InfiniteNat => itf::Value::Unserializable(
                serde_json::from_value(serde_json::json!({"#unserializable": "Nat"})).unwrap(),
            ),
            ValueRef::Set(_)
            | ValueRef::Interval(_, _)
            | ValueRef::CrossProduct(_)
            | ValueRef::PowerSet(_)
            | ValueRef::MapSet(_, _) => {
                let set = self
                    .as_set_iter()
                    .expect("can't convert value to set for ITF conversion");
                itf::Value::Set(set.map(|v| v.to_itf()).collect())
            }
            ValueRef::Tuple(elems) => itf::Value::Tuple(elems.iter().map(|v| v.to_itf()).collect()),
            ValueRef::Record(fields) => itf::Value::Record(
                fields
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_itf()))
                    .collect(),
            ),
            ValueRef::Map(map) => {
                itf::Value::Map(map.iter().map(|(k, v)| (k.to_itf(), v.to_itf())).collect())
            }
            ValueRef::List(elems) => itf::Value::List(elems.iter().map(|v| v.to_itf()).collect()),
            ValueRef::Variant(label, value) => itf::Value::Record(
                vec![
                    ("tag".to_string(), itf::Value::String(label.to_string())),
                    ("value".to_string(), value.to_itf()),
                ]
                .into_iter()
                .collect(),
            ),
            ValueRef::Lambda(_, _) => panic!("Cannot convert Lambda to ITF"),
        }
    }
}

/// Character length of `i` written as a JSON number, including the minus sign.
fn json_number_text_len(i: i64) -> u32 {
    let digits = i.unsigned_abs().checked_ilog10().unwrap_or(0) + 1;
    digits + u32::from(i < 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_bigint::BigInt;

    fn assert_int_json(value: Value, digits: &str) {
        let json = serde_json::to_string(&value.to_itf()).unwrap();
        let expected = if digits.len() <= 15 {
            digits.to_string()
        } else {
            format!("{{\"#bigint\":\"{digits}\"}}")
        };
        assert_eq!(json, expected, "{digits}");
    }

    #[test]
    fn integers_longer_than_15_json_chars_are_bigint() {
        // 15 characters stay JSON numbers. 16 or more, counting '-', are #bigint,
        // because json-bigint revives those tokens as bignumber.js values.
        let i64_cases = [
            ("999999999999999", 999_999_999_999_999i64), // 10^15 - 1
            ("1000000000000000", 1_000_000_000_000_000), // 10^15
            ("9007199254740991", 9_007_199_254_740_991), // 2^53 - 1
            ("9223372036854775807", i64::MAX),           // 2^63 - 1
            ("-99999999999999", -(10i64.pow(14) - 1)),   // -(10^14 - 1), 15 chars
            ("-100000000000000", -(10i64.pow(14))),      // -10^14, 16 chars
            ("-999999999999999", -(10i64.pow(15) - 1)),  // -(10^15 - 1)
            ("-1000000000000000", -(10i64.pow(15))),     // -10^15
            ("-9007199254740991", -(2i64.pow(53) - 1)),  // -(2^53 - 1)
            ("-9223372036854775808", i64::MIN),          // -2^63
        ];
        for (digits, n) in i64_cases {
            assert_int_json(Value::int(n), digits);
        }
        assert_int_json(
            Value::bigint(BigInt::from(i64::MAX) + 1),
            "9223372036854775808",
        );
    }
}
