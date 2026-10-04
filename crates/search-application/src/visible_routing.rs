//! 現在のactorに可視な登録だけを、既存のSourceRouterへ渡す境界。

use std::collections::BTreeSet;

use search_core::discovery::{GapReason, InformationGap};
use search_core::source::DiscoverableSource;

use crate::SearchError;
use crate::routing::RoutingConstraints;
use crate::scoped::{TrustedSearchScope, VisibleSourceRegistration};

pub struct VisibleRouting;

impl VisibleRouting {
    /// 現在の認可の再照会は呼出側が行う。本処理は同じactorに結び付いた
    /// 可視snapshotを検査し、未知・不可視のIDをrouterへ渡さない。
    pub fn prepare(
        actor: &TrustedSearchScope,
        visible: &[VisibleSourceRegistration],
        requested: &RoutingConstraints,
    ) -> Result<
        (
            Vec<DiscoverableSource>,
            RoutingConstraints,
            Vec<InformationGap>,
        ),
        SearchError,
    > {
        let unavailable = || SearchError::OperationFailed("visible routing unavailable".into());
        if !actor.is_live() {
            return Err(unavailable());
        }
        let mut ids = BTreeSet::new();
        let mut sources = Vec::with_capacity(visible.len());
        for entry in visible {
            if entry.scope().actor() != actor || !ids.insert(entry.scope().source_id()) {
                return Err(unavailable());
            }
            sources.push(entry.discoverable_source());
        }
        let keep_visible = |input: &[_]| {
            input
                .iter()
                .copied()
                .filter(|id| ids.contains(id))
                .collect()
        };
        let constraints = RoutingConstraints {
            required_source_ids: keep_visible(&requested.required_source_ids),
            preferred_source_ids: keep_visible(&requested.preferred_source_ids),
            max_initial_optional_sources: requested.max_initial_optional_sources,
        };
        let gaps = if requested
            .required_source_ids
            .iter()
            .any(|id| !ids.contains(id))
        {
            vec![InformationGap::new(
                "required_source_unavailable",
                GapReason::Availability,
                true,
            )]
        } else {
            Vec::new()
        };
        Ok((sources, constraints, gaps))
    }
}
