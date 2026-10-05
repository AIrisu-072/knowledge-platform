//! Explicit ZIP containers. Every member chain is dispatched only through the
//! trusted composite profile plan; an unregistered member, an unused plan node
//! or any unsupported leaf makes the whole container yield zero Units.

use std::collections::{BTreeMap, BTreeSet};

use search_core::knowledge_unit::{ArchiveProfilePlan, ArchiveReaderNode, FormatId, NativeLocator};
use search_extraction_core::{BudgetMeter, WorkerReport};

use super::ooxml::zip_parts;
use super::{Body, ReadResult, read_leaf, resource_limit, structure, unsupported};

const MAX_MEMBER_CHAIN: usize = 3;

pub(super) fn read(
    raw: &[u8],
    plan: &ArchiveProfilePlan,
    meter: &mut BudgetMeter,
) -> ReadResult<WorkerReport> {
    let nodes: BTreeMap<&[String], &ArchiveReaderNode> = plan
        .nodes
        .iter()
        .map(|node| (node.members.as_slice(), node))
        .collect();
    match nodes.get(&[][..]) {
        Some(root) if root.definition.format == FormatId::Zip => {}
        _ => return Err(structure()),
    }
    let mut visited = BTreeSet::from([Vec::new()]);
    let mut body = Body::default();
    container(raw, &[], 1, &nodes, &mut visited, &mut body, meter)?;
    if visited.len() != plan.nodes.len() {
        return Err(structure());
    }
    let mut report = body.into_report()?;
    report.reader_use = plan.nodes.clone();
    Ok(report)
}

fn container(
    raw: &[u8],
    members: &[String],
    depth: u64,
    nodes: &BTreeMap<&[String], &ArchiveReaderNode>,
    visited: &mut BTreeSet<Vec<String>>,
    body: &mut Body,
    meter: &mut BudgetMeter,
) -> ReadResult<()> {
    for (name, data) in zip_parts(raw, meter, depth)? {
        body.visit(1)?;
        let mut chain = members.to_vec();
        chain.push(name);
        if chain.len() > MAX_MEMBER_CHAIN {
            return Err(resource_limit());
        }
        let node = nodes.get(chain.as_slice()).ok_or_else(structure)?;
        visited.insert(chain.clone());
        let definition = &node.definition;
        if definition.format == FormatId::Zip {
            container(&data, &chain, depth + 1, nodes, visited, body, meter)?;
            continue;
        }
        let leaf = read_leaf(
            definition.format,
            &definition.format_settings,
            &data,
            meter,
            depth,
        )?;
        if leaf.units.is_empty()
            && let Some(reason) = leaf.reasons.first()
        {
            return Err(unsupported(*reason));
        }
        body.scope_items = body
            .scope_items
            .checked_add(leaf.scope_items)
            .ok_or_else(resource_limit)?;
        let prefix = chain.join("/");
        for omission in leaf.omissions {
            let package_path = match omission.package_path {
                Some(path) => format!("{prefix}/{path}"),
                None => prefix.clone(),
            };
            body.omit(
                Some(package_path),
                omission.physical_child_path,
                omission.reason,
            );
        }
        for unit in leaf.units {
            body.units.push(super::Unit {
                kind: unit.kind,
                text: unit.text,
                locator: NativeLocator::Archive {
                    members: chain.clone(),
                    inner: Box::new(unit.locator),
                },
            });
        }
    }
    Ok(())
}
