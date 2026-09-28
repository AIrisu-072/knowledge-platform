use search_core::fact::{Fact, FactOrigin, FactSet};
use search_core::predicate::{
    ConceptResolver, DecimalValue, MAX_PREDICATE_DEPTH, MoneyValue, Operand, PredicateEvaluator,
    PredicateExpr, QuantityValue, TruthValue, TypedValue,
};
use time::{Date, Month};

fn fact(name: &str) -> Operand {
    Operand::Fact(name.into())
}

fn value(value: TypedValue) -> Operand {
    Operand::Value(value)
}

struct Concepts;

impl ConceptResolver for Concepts {
    fn same_concept(&self, left: &str, right: &str) -> TruthValue {
        match (left, right) {
            ("corporation", "legal-entity") => TruthValue::True,
            ("same-label", "same-label") => TruthValue::False,
            _ => TruthValue::Unknown,
        }
    }

    fn is_a(&self, child: &str, parent: &str) -> TruthValue {
        if (child, parent) == ("business-loan", "loan") {
            TruthValue::True
        } else {
            TruthValue::False
        }
    }

    fn descendant_of(&self, child: &str, ancestor: &str) -> TruthValue {
        if (child, ancestor) == ("business-loan", "finance") {
            TruthValue::True
        } else {
            TruthValue::False
        }
    }
}

fn evaluate(expr: &PredicateExpr, facts: &FactSet) -> TruthValue {
    PredicateEvaluator::evaluate(expr, facts, &Concepts)
}

#[test]
fn missing_required_fact_is_unknown() {
    let expr = PredicateExpr::Eq(
        fact("customer.kind"),
        value(TypedValue::String("法人".into())),
    );
    assert_eq!(evaluate(&expr, &FactSet::default()), TruthValue::Unknown);
}

#[test]
fn comparison_with_wrong_type_is_error() {
    let mut facts = FactSet::default();
    facts.insert(
        "customer.kind",
        Fact::new(TypedValue::Integer(42), FactOrigin::Explicit),
    );
    let expr = PredicateExpr::Eq(
        fact("customer.kind"),
        value(TypedValue::String("法人".into())),
    );
    assert_eq!(evaluate(&expr, &facts), TruthValue::Error);
}

#[test]
fn known_mismatch_is_false() {
    let mut facts = FactSet::default();
    facts.insert(
        "product.kind",
        Fact::new(
            TypedValue::String("deposit".into()),
            FactOrigin::Authoritative,
        ),
    );
    let expr = PredicateExpr::Eq(
        fact("product.kind"),
        value(TypedValue::String("loan".into())),
    );
    assert_eq!(evaluate(&expr, &facts), TruthValue::False);
}

#[test]
fn and_or_not_preserve_unknown_without_treating_it_as_false() {
    let unknown = PredicateExpr::Eq(fact("missing"), value(TypedValue::Bool(true)));
    let known_true =
        PredicateExpr::Eq(value(TypedValue::Bool(true)), value(TypedValue::Bool(true)));
    let known_false = PredicateExpr::Eq(
        value(TypedValue::Bool(true)),
        value(TypedValue::Bool(false)),
    );
    let facts = FactSet::default();

    assert_eq!(
        evaluate(
            &PredicateExpr::And(vec![unknown.clone(), known_true.clone()]),
            &facts
        ),
        TruthValue::Unknown
    );
    assert_eq!(
        evaluate(
            &PredicateExpr::And(vec![unknown.clone(), known_false.clone()]),
            &facts
        ),
        TruthValue::False
    );
    assert_eq!(
        evaluate(
            &PredicateExpr::Or(vec![unknown.clone(), known_false]),
            &facts
        ),
        TruthValue::Unknown
    );
    assert_eq!(
        evaluate(
            &PredicateExpr::Or(vec![unknown.clone(), known_true]),
            &facts
        ),
        TruthValue::True
    );
    assert_eq!(
        evaluate(&PredicateExpr::Not(Box::new(unknown)), &facts),
        TruthValue::Unknown
    );
}

#[test]
fn missing_operator_distinguishes_absence_from_known_false() {
    let mut facts = FactSet::default();
    facts.insert(
        "flag",
        Fact::new(TypedValue::Bool(false), FactOrigin::Explicit),
    );
    assert_eq!(
        evaluate(&PredicateExpr::Missing("flag".into()), &facts),
        TruthValue::False
    );
    assert_eq!(
        evaluate(&PredicateExpr::Exists("flag".into()), &facts),
        TruthValue::True
    );
    assert_eq!(
        evaluate(&PredicateExpr::Missing("other".into()), &facts),
        TruthValue::True
    );
}

#[test]
fn money_comparison_preserves_integer_precision_above_f64_limit() {
    let lesser = TypedValue::Money(MoneyValue::new(9_007_199_254_740_993, "JPY"));
    let greater = TypedValue::Money(MoneyValue::new(9_007_199_254_740_994, "JPY"));
    let expr = PredicateExpr::Lt(value(lesser), value(greater));
    assert_eq!(evaluate(&expr, &FactSet::default()), TruthValue::True);
}

#[test]
fn money_comparison_rejects_mismatched_currencies() {
    let yen = TypedValue::Money(MoneyValue::new(100, "JPY"));
    let usd = TypedValue::Money(MoneyValue::new(100, "USD"));
    let expr = PredicateExpr::Eq(value(yen), value(usd));
    assert_eq!(evaluate(&expr, &FactSet::default()), TruthValue::Error);
}

#[test]
fn ordered_comparisons_use_exact_decimal_and_matching_quantity_units() {
    let lesser = TypedValue::Decimal(DecimalValue::new(120, 2));
    let greater = TypedValue::Decimal(DecimalValue::new(13, 1));
    let facts = FactSet::default();
    assert_eq!(
        evaluate(
            &PredicateExpr::Lte(value(lesser.clone()), value(greater.clone())),
            &facts
        ),
        TruthValue::True
    );
    assert_eq!(
        evaluate(&PredicateExpr::Gt(value(greater), value(lesser)), &facts),
        TruthValue::True
    );

    let meters = TypedValue::Quantity(QuantityValue::new(DecimalValue::new(250, 2), "m"));
    let centimeters = TypedValue::Quantity(QuantityValue::new(DecimalValue::new(250, 2), "cm"));
    assert_eq!(
        evaluate(
            &PredicateExpr::Gte(value(meters), value(centimeters)),
            &facts
        ),
        TruthValue::Error
    );
}

#[test]
fn date_comparison_and_inequality_keep_their_types() {
    let early = Date::from_calendar_date(2026, Month::September, 1).unwrap();
    let late = Date::from_calendar_date(2026, Month::September, 29).unwrap();
    let facts = FactSet::default();
    assert_eq!(
        evaluate(
            &PredicateExpr::Lt(
                value(TypedValue::Date(early)),
                value(TypedValue::Date(late))
            ),
            &facts
        ),
        TruthValue::True
    );
    assert_eq!(
        evaluate(
            &PredicateExpr::Ne(
                value(TypedValue::Date(early)),
                value(TypedValue::Date(late))
            ),
            &facts
        ),
        TruthValue::True
    );
}

#[test]
fn collection_operators_follow_membership_intersection_and_subset_semantics() {
    let loan = TypedValue::String("loan".into());
    let deposit = TypedValue::String("deposit".into());
    let all = TypedValue::Set(vec![loan.clone(), deposit.clone()]);
    let one = TypedValue::Set(vec![loan.clone()]);
    let facts = FactSet::default();

    assert_eq!(
        evaluate(
            &PredicateExpr::In(value(loan.clone()), value(all.clone())),
            &facts
        ),
        TruthValue::True
    );
    assert_eq!(
        evaluate(
            &PredicateExpr::Contains(value(all.clone()), value(deposit)),
            &facts
        ),
        TruthValue::True
    );
    assert_eq!(
        evaluate(
            &PredicateExpr::Intersects(value(one.clone()), value(all.clone())),
            &facts
        ),
        TruthValue::True
    );
    assert_eq!(
        evaluate(&PredicateExpr::Subset(value(one), value(all)), &facts),
        TruthValue::True
    );
}

#[test]
fn set_equality_ignores_duplicate_representations() {
    let item = TypedValue::String("loan".into());
    let duplicated = TypedValue::Set(vec![item.clone(), item.clone()]);
    let unique = TypedValue::Set(vec![item]);
    assert_eq!(
        evaluate(
            &PredicateExpr::Eq(value(duplicated), value(unique)),
            &FactSet::default(),
        ),
        TruthValue::True,
    );
}

#[test]
fn nested_collection_comparison_has_a_depth_limit() {
    let mut left = TypedValue::String("loan".into());
    let mut right = left.clone();
    for _ in 0..=MAX_PREDICATE_DEPTH {
        left = TypedValue::List(vec![left]);
        right = TypedValue::List(vec![right]);
    }
    assert_eq!(
        evaluate(
            &PredicateExpr::Eq(value(left), value(right)),
            &FactSet::default(),
        ),
        TruthValue::Error,
    );
}

#[test]
fn inferred_fact_origin_remains_available_to_evidence_policy() {
    let mut facts = FactSet::default();
    facts.insert(
        "customer.kind",
        Fact::new(TypedValue::String("法人".into()), FactOrigin::Inferred),
    );
    let observed = facts.get("customer.kind").unwrap();
    assert_eq!(observed.origin, FactOrigin::Inferred);
    let expr = PredicateExpr::Eq(
        fact("customer.kind"),
        value(TypedValue::String("法人".into())),
    );
    assert_eq!(evaluate(&expr, &facts), TruthValue::True);
}

#[test]
fn concept_operators_use_resolver_semantics_instead_of_string_similarity() {
    let concepts = Concepts;
    let facts = FactSet::default();
    let concept = |id: &str| value(TypedValue::ConceptRef(id.into()));

    assert_eq!(
        PredicateEvaluator::evaluate(
            &PredicateExpr::SameConcept(concept("corporation"), concept("legal-entity")),
            &facts,
            &concepts,
        ),
        TruthValue::True
    );
    assert_eq!(
        PredicateEvaluator::evaluate(
            &PredicateExpr::SameConcept(concept("same-label"), concept("same-label")),
            &facts,
            &concepts,
        ),
        TruthValue::False
    );
    assert_eq!(
        PredicateEvaluator::evaluate(
            &PredicateExpr::IsA(concept("business-loan"), concept("loan")),
            &facts,
            &concepts,
        ),
        TruthValue::True
    );
    assert_eq!(
        PredicateEvaluator::evaluate(
            &PredicateExpr::DescendantOf(concept("business-loan"), concept("finance")),
            &facts,
            &concepts,
        ),
        TruthValue::True
    );
}

#[test]
fn expression_depth_is_bounded() {
    let mut expr = PredicateExpr::Exists("present".into());
    for _ in 0..=MAX_PREDICATE_DEPTH {
        expr = PredicateExpr::Not(Box::new(expr));
    }
    assert_eq!(evaluate(&expr, &FactSet::default()), TruthValue::Error);
}
