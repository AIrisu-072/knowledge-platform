//! Static HTML body text with full child-index `text_node_path` locators.
//! Script, style, template, hidden or styled content cannot be located as a
//! known omission, so any of it makes the whole item `DynamicVisibility`.

use html5ever::{parse_document, tendril::TendrilSink};
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use search_core::knowledge_unit::{BudgetKey, NativeLocator, UnitKind};
use search_extraction_core::{BudgetMeter, CoverageReason};

use super::{Body, ReadResult, corrupt, resource_limit, unsupported};

const MAX_DEPTH: usize = 256;

pub(super) fn read(raw: &[u8], meter: &mut BudgetMeter) -> ReadResult<Body> {
    let source =
        std::str::from_utf8(raw).map_err(|_| unsupported(CoverageReason::UnsupportedEncoding))?;
    let dom = parse_document(RcDom::default(), Default::default()).one(source);
    let body_node = find_body(&dom.document, 0)?.ok_or_else(corrupt)?;
    let mut state = Walk {
        body: Body::default(),
        omitted: false,
    };
    for (index, child) in body_node.children.borrow().iter().enumerate() {
        let path = vec![u32::try_from(index).map_err(|_| resource_limit())?];
        walk(child, path, false, false, &mut state, meter, 1)?;
    }
    if state.omitted {
        return Err(unsupported(CoverageReason::DynamicVisibility));
    }
    Ok(state.body)
}

fn find_body(handle: &Handle, depth: usize) -> ReadResult<Option<Handle>> {
    if depth > MAX_DEPTH {
        return Err(resource_limit());
    }
    if let NodeData::Element { name, .. } = &handle.data
        && name.local.as_ref() == "body"
    {
        return Ok(Some(handle.clone()));
    }
    for child in handle.children.borrow().iter() {
        if let Some(found) = find_body(child, depth + 1)? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

struct Walk {
    body: Body,
    omitted: bool,
}

fn walk(
    handle: &Handle,
    path: Vec<u32>,
    heading: bool,
    hidden: bool,
    state: &mut Walk,
    meter: &mut BudgetMeter,
    depth: usize,
) -> ReadResult<()> {
    meter.charge(BudgetKey::HtmlNodes, 1)?;
    if depth > MAX_DEPTH {
        return Err(resource_limit());
    }
    state.body.visit(1)?;
    let mut heading = heading;
    let mut hidden = hidden;
    match &handle.data {
        NodeData::Element { name, attrs, .. } => {
            let tag = name.local.as_ref();
            if matches!(tag, "script" | "style" | "template") {
                state.omitted = true;
                return Ok(());
            }
            let attrs = attrs.borrow();
            if tag == "link"
                && attrs.iter().any(|attr| {
                    attr.name.local.as_ref() == "rel" && attr.value.as_ref() == "stylesheet"
                })
            {
                state.omitted = true;
            }
            if attrs
                .iter()
                .any(|attr| matches!(attr.name.local.as_ref(), "style" | "hidden"))
            {
                state.omitted = true;
                hidden = true;
            }
            heading |= matches!(tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6");
        }
        NodeData::Text { contents } => {
            let value = contents.borrow();
            if !hidden && !value.trim().is_empty() {
                state.body.unit(
                    meter,
                    if heading {
                        UnitKind::Heading
                    } else {
                        UnitKind::HtmlText
                    },
                    &value,
                    NativeLocator::Html {
                        text_node_path: path,
                    },
                )?;
            }
            return Ok(());
        }
        _ => {}
    }
    for (index, child) in handle.children.borrow().iter().enumerate() {
        let mut next = path.clone();
        next.push(u32::try_from(index).map_err(|_| resource_limit())?);
        walk(child, next, heading, hidden, state, meter, depth + 1)?;
    }
    Ok(())
}
