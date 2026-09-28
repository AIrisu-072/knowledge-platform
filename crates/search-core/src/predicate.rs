//! Bounded, deterministic evaluation of typed applicability predicates.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use time::{Date, Duration, OffsetDateTime};

use crate::fact::FactSet;
use crate::id::ResourceId;

/// Maximum number of expression edges between the root and a leaf.
pub const MAX_PREDICATE_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecimalValue {
    pub coefficient: i128,
    pub scale: u8,
}

impl DecimalValue {
    pub const fn new(coefficient: i128, scale: u8) -> Self {
        Self { coefficient, scale }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoneyValue {
    pub amount_minor: i128,
    pub currency: String,
}

impl MoneyValue {
    pub fn new(amount_minor: i128, currency: impl Into<String>) -> Self {
        Self {
            amount_minor,
            currency: currency.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuantityValue {
    pub amount: DecimalValue,
    pub unit: String,
}

impl QuantityValue {
    pub fn new(amount: DecimalValue, unit: impl Into<String>) -> Self {
        Self {
            amount,
            unit: unit.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypedValue {
    Bool(bool),
    String(String),
    Integer(i128),
    Decimal(DecimalValue),
    Date(Date),
    DateTime(OffsetDateTime),
    Duration(Duration),
    ConceptRef(String),
    ResourceRef(ResourceId),
    List(Vec<TypedValue>),
    Set(Vec<TypedValue>),
    Money(MoneyValue),
    Quantity(QuantityValue),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Operand {
    Fact(String),
    Value(TypedValue),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PredicateExpr {
    And(Vec<Self>),
    Or(Vec<Self>),
    Not(Box<Self>),
    Eq(Operand, Operand),
    Ne(Operand, Operand),
    Lt(Operand, Operand),
    Lte(Operand, Operand),
    Gt(Operand, Operand),
    Gte(Operand, Operand),
    In(Operand, Operand),
    Contains(Operand, Operand),
    Intersects(Operand, Operand),
    Subset(Operand, Operand),
    Exists(String),
    Missing(String),
    SameConcept(Operand, Operand),
    IsA(Operand, Operand),
    DescendantOf(Operand, Operand),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TruthValue {
    True,
    False,
    Unknown,
    Error,
}

impl TruthValue {
    fn from_bool(value: bool) -> Self {
        if value { Self::True } else { Self::False }
    }

    fn not(self) -> Self {
        match self {
            Self::True => Self::False,
            Self::False => Self::True,
            other => other,
        }
    }
}

/// The caller supplies a pinned concept view; this evaluator performs no I/O.
pub trait ConceptResolver {
    fn same_concept(&self, left: &str, right: &str) -> TruthValue;
    fn is_a(&self, child: &str, parent: &str) -> TruthValue;
    fn descendant_of(&self, child: &str, ancestor: &str) -> TruthValue;
}

pub struct PredicateEvaluator;

impl PredicateEvaluator {
    pub fn evaluate(
        expression: &PredicateExpr,
        facts: &FactSet,
        concepts: &dyn ConceptResolver,
    ) -> TruthValue {
        Self::at_depth(expression, facts, concepts, 0)
    }

    fn at_depth(
        expression: &PredicateExpr,
        facts: &FactSet,
        concepts: &dyn ConceptResolver,
        depth: usize,
    ) -> TruthValue {
        if depth > MAX_PREDICATE_DEPTH {
            return TruthValue::Error;
        }

        match expression {
            PredicateExpr::And(children) => {
                if children.is_empty() {
                    return TruthValue::Error;
                }
                let values = children
                    .iter()
                    .map(|child| Self::at_depth(child, facts, concepts, depth + 1));
                combine_and(values)
            }
            PredicateExpr::Or(children) => {
                if children.is_empty() {
                    return TruthValue::Error;
                }
                let values = children
                    .iter()
                    .map(|child| Self::at_depth(child, facts, concepts, depth + 1));
                combine_or(values)
            }
            PredicateExpr::Not(child) => Self::at_depth(child, facts, concepts, depth + 1).not(),
            PredicateExpr::Exists(key) => TruthValue::from_bool(facts.contains(key)),
            PredicateExpr::Missing(key) => TruthValue::from_bool(!facts.contains(key)),
            PredicateExpr::Eq(left, right) => binary(left, right, facts, values_equal),
            PredicateExpr::Ne(left, right) => {
                binary(left, right, facts, |a, b| values_equal(a, b).not())
            }
            PredicateExpr::Lt(left, right) => ordered(left, right, facts, |o| o.is_lt()),
            PredicateExpr::Lte(left, right) => ordered(left, right, facts, |o| o.is_le()),
            PredicateExpr::Gt(left, right) => ordered(left, right, facts, |o| o.is_gt()),
            PredicateExpr::Gte(left, right) => ordered(left, right, facts, |o| o.is_ge()),
            PredicateExpr::In(needle, haystack) => binary(needle, haystack, facts, membership),
            PredicateExpr::Contains(haystack, needle) => {
                binary(haystack, needle, facts, |a, b| membership(b, a))
            }
            PredicateExpr::Intersects(left, right) => {
                binary(left, right, facts, |a, b| collection_relation(a, b, true))
            }
            PredicateExpr::Subset(left, right) => {
                binary(left, right, facts, |a, b| collection_relation(a, b, false))
            }
            PredicateExpr::SameConcept(left, right) => {
                concept_binary(left, right, facts, |a, b| concepts.same_concept(a, b))
            }
            PredicateExpr::IsA(left, right) => {
                concept_binary(left, right, facts, |a, b| concepts.is_a(a, b))
            }
            PredicateExpr::DescendantOf(left, right) => {
                concept_binary(left, right, facts, |a, b| concepts.descendant_of(a, b))
            }
        }
    }
}

fn combine_and(values: impl Iterator<Item = TruthValue>) -> TruthValue {
    let mut result = TruthValue::True;
    for value in values {
        result = match (result, value) {
            (TruthValue::Error, _) | (_, TruthValue::Error) => TruthValue::Error,
            (TruthValue::False, _) | (_, TruthValue::False) => TruthValue::False,
            (TruthValue::Unknown, _) | (_, TruthValue::Unknown) => TruthValue::Unknown,
            _ => TruthValue::True,
        };
    }
    result
}

fn combine_or(values: impl Iterator<Item = TruthValue>) -> TruthValue {
    let mut result = TruthValue::False;
    for value in values {
        result = match (result, value) {
            (TruthValue::Error, _) | (_, TruthValue::Error) => TruthValue::Error,
            (TruthValue::True, _) | (_, TruthValue::True) => TruthValue::True,
            (TruthValue::Unknown, _) | (_, TruthValue::Unknown) => TruthValue::Unknown,
            _ => TruthValue::False,
        };
    }
    result
}

fn resolve<'a>(operand: &'a Operand, facts: &'a FactSet) -> Option<&'a TypedValue> {
    match operand {
        Operand::Fact(key) => facts.get(key).map(|fact| &fact.value),
        Operand::Value(value) => Some(value),
    }
}

fn binary(
    left: &Operand,
    right: &Operand,
    facts: &FactSet,
    compare: impl FnOnce(&TypedValue, &TypedValue) -> TruthValue,
) -> TruthValue {
    match (resolve(left, facts), resolve(right, facts)) {
        (Some(a), Some(b)) => compare(a, b),
        _ => TruthValue::Unknown,
    }
}

fn ordered(
    left: &Operand,
    right: &Operand,
    facts: &FactSet,
    accepts: impl FnOnce(Ordering) -> bool,
) -> TruthValue {
    binary(left, right, facts, |a, b| match compare_values(a, b) {
        Some(order) => TruthValue::from_bool(accepts(order)),
        None => TruthValue::Error,
    })
}

fn concept_binary(
    left: &Operand,
    right: &Operand,
    facts: &FactSet,
    evaluate: impl FnOnce(&str, &str) -> TruthValue,
) -> TruthValue {
    binary(left, right, facts, |a, b| match (a, b) {
        (TypedValue::ConceptRef(a), TypedValue::ConceptRef(b)) => evaluate(a, b),
        _ => TruthValue::Error,
    })
}

fn values_equal(left: &TypedValue, right: &TypedValue) -> TruthValue {
    values_equal_at_depth(left, right, 0)
}

fn values_equal_at_depth(left: &TypedValue, right: &TypedValue, depth: usize) -> TruthValue {
    if depth > MAX_PREDICATE_DEPTH {
        return TruthValue::Error;
    }
    match (left, right) {
        (TypedValue::List(a), TypedValue::List(b)) => {
            if a.len() != b.len() {
                return TruthValue::False;
            }
            combine_and(
                a.iter()
                    .zip(b)
                    .map(|(a, b)| values_equal_at_depth(a, b, depth + 1)),
            )
        }
        (TypedValue::Set(a), TypedValue::Set(b)) => {
            let a_in_b = a
                .iter()
                .map(|item| membership_in_slice_at_depth(item, b, depth + 1));
            let b_in_a = b
                .iter()
                .map(|item| membership_in_slice_at_depth(item, a, depth + 1));
            combine_and(a_in_b.chain(b_in_a))
        }
        _ => match compare_values(left, right) {
            Some(order) => TruthValue::from_bool(order.is_eq()),
            None => TruthValue::Error,
        },
    }
}

fn compare_values(left: &TypedValue, right: &TypedValue) -> Option<Ordering> {
    match (left, right) {
        (TypedValue::Bool(a), TypedValue::Bool(b)) => Some(a.cmp(b)),
        (TypedValue::String(a), TypedValue::String(b)) => Some(a.cmp(b)),
        (TypedValue::Integer(a), TypedValue::Integer(b)) => Some(a.cmp(b)),
        (TypedValue::Decimal(a), TypedValue::Decimal(b)) => Some(compare_decimal(a, b)),
        (TypedValue::Date(a), TypedValue::Date(b)) => Some(a.cmp(b)),
        (TypedValue::DateTime(a), TypedValue::DateTime(b)) => Some(a.cmp(b)),
        (TypedValue::Duration(a), TypedValue::Duration(b)) => Some(a.cmp(b)),
        (TypedValue::ConceptRef(a), TypedValue::ConceptRef(b)) => Some(a.cmp(b)),
        (TypedValue::ResourceRef(a), TypedValue::ResourceRef(b)) => Some(a.cmp(b)),
        (TypedValue::Money(a), TypedValue::Money(b)) if a.currency == b.currency => {
            Some(a.amount_minor.cmp(&b.amount_minor))
        }
        (TypedValue::Quantity(a), TypedValue::Quantity(b)) if a.unit == b.unit => {
            Some(compare_decimal(&a.amount, &b.amount))
        }
        _ => None,
    }
}

fn compare_decimal(left: &DecimalValue, right: &DecimalValue) -> Ordering {
    if left.coefficient == 0 && right.coefficient == 0 {
        return Ordering::Equal;
    }
    let left_negative = left.coefficient < 0;
    let right_negative = right.coefficient < 0;
    if left_negative != right_negative {
        return if left_negative {
            Ordering::Less
        } else {
            Ordering::Greater
        };
    }

    fn parts(value: &DecimalValue) -> (String, String) {
        let digits = value.coefficient.unsigned_abs().to_string();
        let scale = usize::from(value.scale);
        let padded = format!(
            "{}{}",
            "0".repeat((scale + 1).saturating_sub(digits.len())),
            digits
        );
        let split = padded.len() - scale;
        (padded[..split].to_owned(), padded[split..].to_owned())
    }

    let (left_integer, left_fraction) = parts(left);
    let (right_integer, right_fraction) = parts(right);
    let order = left_integer
        .len()
        .cmp(&right_integer.len())
        .then_with(|| left_integer.cmp(&right_integer))
        .then_with(|| {
            let width = left_fraction.len().max(right_fraction.len());
            let left = format!("{left_fraction:0<width$}");
            let right = format!("{right_fraction:0<width$}");
            left.cmp(&right)
        });
    if left_negative {
        order.reverse()
    } else {
        order
    }
}

fn collection(value: &TypedValue) -> Option<&[TypedValue]> {
    match value {
        TypedValue::List(values) | TypedValue::Set(values) => Some(values),
        _ => None,
    }
}

fn membership_in_slice(needle: &TypedValue, haystack: &[TypedValue]) -> TruthValue {
    membership_in_slice_at_depth(needle, haystack, 0)
}

fn membership_in_slice_at_depth(
    needle: &TypedValue,
    haystack: &[TypedValue],
    depth: usize,
) -> TruthValue {
    if depth > MAX_PREDICATE_DEPTH {
        return TruthValue::Error;
    }
    let mut found = false;
    let mut error = false;
    for item in haystack {
        match values_equal_at_depth(needle, item, depth + 1) {
            TruthValue::True => found = true,
            TruthValue::Error => error = true,
            _ => {}
        }
    }
    if error {
        TruthValue::Error
    } else {
        TruthValue::from_bool(found)
    }
}

fn membership(needle: &TypedValue, haystack: &TypedValue) -> TruthValue {
    match collection(haystack) {
        Some(values) => membership_in_slice(needle, values),
        None => TruthValue::Error,
    }
}

fn collection_relation(left: &TypedValue, right: &TypedValue, intersects: bool) -> TruthValue {
    let (Some(a), Some(b)) = (collection(left), collection(right)) else {
        return TruthValue::Error;
    };
    let mut values = a.iter().map(|item| membership_in_slice(item, b));
    if intersects {
        combine_or(&mut values)
    } else {
        combine_and(&mut values)
    }
}
