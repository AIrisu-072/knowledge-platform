//! Bounded opaque path semantics. Unknown paint and clipping states stay closed.
use super::*;

#[derive(Clone)]
pub(super) struct GraphicsState {
    fill: [f64; 3],
    stroke: [f64; 3],
    width: f64,
    cap: u8,
    join: u8,
    miter: f64,
    dash: Vec<f64>,
    phase: f64,
}

impl Default for GraphicsState {
    fn default() -> Self {
        Self {
            fill: [0.0; 3],
            stroke: [0.0; 3],
            width: 1.0,
            cap: 0,
            join: 0,
            miter: 10.0,
            dash: Vec::new(),
            phase: 0.0,
        }
    }
}

#[derive(Default)]
pub(super) struct PathState {
    commands: Vec<Value>,
    points: Vec<[f64; 2]>,
    current: bool,
    segments: usize,
    rectangle: Option<[f64; 4]>,
    clipping: bool,
    stroke_bounds: Vec<[f64; 4]>,
    last: Option<[f64; 2]>,
    start: Option<[f64; 2]>,
}

fn unsupported() -> WorkerFailure {
    failure(
        WorkerFailureCode::UnsupportedSemanticConstruct,
        "unsupported bounded PDF graphics state",
    )
}
fn malformed() -> WorkerFailure {
    failure(
        WorkerFailureCode::ParserDisagreement,
        "malformed PDF path or graphics state",
    )
}
fn clean(value: f64) -> f64 {
    if value == 0.0 { 0.0 } else { value }
}
fn point(x: f64, y: f64, matrix: [f64; 6]) -> Result<[f64; 2], WorkerFailure> {
    let p = [
        clean(x * matrix[0] + y * matrix[2] + matrix[4]),
        clean(x * matrix[1] + y * matrix[3] + matrix[5]),
    ];
    if p.iter().all(|v| v.is_finite()) {
        Ok(p)
    } else {
        Err(malformed())
    }
}
fn bounds(points: &[[f64; 2]]) -> [f64; 4] {
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for p in points {
        b[0] = b[0].min(p[0]);
        b[1] = b[1].min(p[1]);
        b[2] = b[2].max(p[0]);
        b[3] = b[3].max(p[1]);
    }
    b
}
fn charge(context: &mut PdfPaintContext<'_>, amount: usize) -> Result<(), WorkerFailure> {
    context.path_segments = context.path_segments.saturating_add(amount);
    if context.path_segments > MAX_PDF_PAINT_EVENTS {
        return Err(failure(
            WorkerFailureCode::InspectionResourceLimitExceeded,
            "PDF path segment budget exceeded",
        ));
    }
    Ok(())
}

pub(super) fn text_is_supported(state: &PdfGraphicsState) -> bool {
    state.extended.fill == [0.0; 3]
        && state.extended.stroke == [0.0; 3]
        && (matches!(state.text_render_mode, 0 | 3)
            || (state.extended.width == 1.0
                && state.extended.cap == 0
                && state.extended.join == 0
                && state.extended.miter == 10.0
                && state.extended.dash.is_empty()))
}

pub(super) fn handle(
    context: &mut PdfPaintContext<'_>,
    operation: &lopdf::content::Operation,
    state: &mut PdfGraphicsState,
    path: &mut PathState,
    resources: Option<&lopdf::Dictionary>,
) -> Result<bool, WorkerFailure> {
    let op = operation.operator.as_str();
    let args = &operation.operands;
    let n = context.page_number;
    validate_resources(context.document, resources)?;
    match op {
        "gs" => validate_extgstate(context, operation, resources)?,
        "g" | "G" | "rg" | "RG" => {
            let values = numeric_values(args, if op.len() == 1 { 1 } else { 3 }, n, "color")?;
            if values.iter().any(|v| !(0.0..=1.0).contains(v)) {
                return Err(unsupported());
            }
            let color = if values.len() == 1 {
                [clean(values[0]); 3]
            } else {
                [clean(values[0]), clean(values[1]), clean(values[2])]
            };
            if op == "g" || op == "rg" {
                state.extended.fill = color;
            } else {
                state.extended.stroke = color;
            }
        }
        "w" | "J" | "j" | "M" => {
            let value = numeric_values(args, 1, n, "stroke state")?[0];
            match op {
                "w" if value > 0.0 => state.extended.width = value,
                "J" if matches!(value, 0.0 | 1.0 | 2.0) => state.extended.cap = value as u8,
                "j" if matches!(value, 0.0 | 1.0 | 2.0) => state.extended.join = value as u8,
                "M" if value >= 1.0 => state.extended.miter = value,
                _ => return Err(unsupported()),
            }
        }
        "d" => {
            let [Object::Array(array), phase] = args.as_slice() else {
                return Err(malformed());
            };
            if array.len() > MAX_PDF_OBJECT_DEPTH {
                return Err(unsupported());
            }
            let values = numeric_values(array, array.len(), n, "dash")?;
            let phase = numeric_values(std::slice::from_ref(phase), 1, n, "dash phase")?[0];
            if phase < 0.0
                || values.iter().any(|v| *v < 0.0)
                || (!values.is_empty() && values.iter().all(|v| *v == 0.0))
            {
                return Err(unsupported());
            }
            state.extended.dash = values.into_iter().map(clean).collect();
            state.extended.phase = if state.extended.dash.is_empty() {
                0.0
            } else {
                clean(phase)
            };
        }
        "m" | "l" | "c" => {
            let values = numeric_values(args, if op == "c" { 6 } else { 2 }, n, "path")?;
            if op != "m" && !path.current {
                return Err(malformed());
            }
            charge(context, 1)?;
            let points = values
                .chunks_exact(2)
                .map(|p| point(p[0], p[1], state.ctm))
                .collect::<Result<Vec<_>, _>>()?;
            if op == "m" {
                path.start = Some(points[0]);
            } else {
                let mut segment = vec![path.last.ok_or_else(malformed)?];
                segment.extend_from_slice(&points);
                path.stroke_bounds.push(bounds(&segment));
            }
            path.last = points.last().copied();
            path.points.extend_from_slice(&points);
            path.commands.push(json!([op, points]));
            path.current = true;
            if op != "m" {
                path.segments += 1;
            }
            path.rectangle = None;
        }
        "re" => {
            let v = numeric_values(args, 4, n, "rectangle")?;
            charge(context, 4)?;
            let points = [
                [v[0], v[1]],
                [v[0] + v[2], v[1]],
                [v[0] + v[2], v[1] + v[3]],
                [v[0], v[1] + v[3]],
            ]
            .into_iter()
            .map(|p| point(p[0], p[1], state.ctm))
            .collect::<Result<Vec<_>, _>>()?;
            let axis_aligned = (state.ctm[1] == 0.0 && state.ctm[2] == 0.0)
                || (state.ctm[0] == 0.0 && state.ctm[3] == 0.0);
            path.rectangle =
                if path.commands.is_empty() && axis_aligned && v[2] != 0.0 && v[3] != 0.0 {
                    Some(bounds(&points))
                } else {
                    None
                };
            path.commands.push(json!(["m", [points[0]]]));
            for p in points.iter().skip(1) {
                path.commands.push(json!(["l", [p]]));
            }
            path.commands.push(json!(["h", []]));
            for index in 0..4 {
                path.stroke_bounds
                    .push(bounds(&[points[index], points[(index + 1) % 4]]));
            }
            path.start = Some(points[0]);
            path.last = Some(points[0]);
            path.points.extend(points);
            path.current = true;
            path.segments += 4;
        }
        "h" if args.is_empty() && path.current => {
            charge(context, 1)?;
            path.stroke_bounds.push(bounds(&[
                path.last.ok_or_else(malformed)?,
                path.start.ok_or_else(malformed)?,
            ]));
            path.last = path.start;
            path.commands.push(json!(["h", []]));
            path.segments += 1;
            path.rectangle = None;
        }
        "W" | "W*" if args.is_empty() => {
            if path.clipping || path.rectangle.is_none() {
                return Err(unsupported());
            }
            path.clipping = true;
        }
        "S" | "f" | "F" | "f*" | "B" | "B*" | "n" if args.is_empty() => {
            if op != "n" && path.segments > 0 {
                if !state.clips.is_empty() {
                    return Err(unsupported());
                }
                ensure_image_paint_slot(context.vector_paints.len(), n)?;
                let stroke = matches!(op, "S" | "B" | "B*");
                let fill = op != "S";
                if fill {
                    context.vector_bounds.push(bounds(&path.points));
                }
                if stroke {
                    // Sum of transformed basis lengths bounds the operator norm.
                    let scale = state.ctm[0].hypot(state.ctm[1]) + state.ctm[2].hypot(state.ctm[3]);
                    let mut join = if state.extended.join == 0 {
                        state.extended.miter
                    } else {
                        1.0
                    };
                    if state.extended.cap == 2 {
                        join = join.max(std::f64::consts::SQRT_2);
                    }
                    let margin = state.extended.width * 0.5 * join * scale;
                    for segment in &path.stroke_bounds {
                        let b = [
                            segment[0] - margin,
                            segment[1] - margin,
                            segment[2] + margin,
                            segment[3] + margin,
                        ];
                        if !b.iter().all(|v| v.is_finite()) {
                            return Err(malformed());
                        }
                        context.vector_bounds.push(b);
                    }
                }
                context.vector_paints.push(json!({"path": path.commands,
                    "fill": if fill { Some(state.extended.fill) } else { None },
                    "fill_rule": if fill { Some(if op.ends_with('*') { "evenodd" } else { "nonzero" }) } else { None },
                    "stroke": if stroke { Some(json!({"color":state.extended.stroke,"width":state.extended.width,
                        "cap":state.extended.cap,"join":state.extended.join,"miter":state.extended.miter,
                        "dash":state.extended.dash,"phase":state.extended.phase,
                        "matrix":normalize_matrix([state.ctm[0],state.ctm[1],state.ctm[2],state.ctm[3],0.0,0.0])})) } else { None }
                }));
                context.paint_order.push("vector");
            }
            if path.clipping {
                context
                    .safety_clips
                    .push(path.rectangle.ok_or_else(unsupported)?);
            }
            *path = PathState::default();
        }
        _ => return Ok(false),
    }
    Ok(true)
}

pub(super) fn finish(path: &PathState) -> Result<(), WorkerFailure> {
    if path.commands.is_empty() && !path.clipping {
        Ok(())
    } else {
        Err(malformed())
    }
}

pub(super) fn validate_clip_bounds(
    page: &PdfPage<'_>,
    clips: &[[f64; 4]],
    vector_bounds: &[[f64; 4]],
) -> Result<(), WorkerFailure> {
    if clips.is_empty() && vector_bounds.is_empty() {
        return Ok(());
    }
    let mut painted_bounds = vector_bounds.to_vec();
    let mut overlap_checks = 0usize;
    for object in page.objects().iter() {
        if object
            .as_text_object()
            .is_some_and(|text| text.text().trim().is_empty())
        {
            continue;
        }
        let bounds = object.bounds().map_err(|_| malformed())?.to_rect();
        let b = [
            f64::from(bounds.left().value),
            f64::from(bounds.bottom().value),
            f64::from(bounds.right().value),
            f64::from(bounds.top().value),
        ];
        if !b.iter().all(|v| v.is_finite()) {
            return Err(malformed());
        }
        // Text grouping is intentionally stable for legacy input. Until exact
        // text-to-vector associations are qualified, overlap cannot be accepted.
        if object.as_text_object().is_some() || object.as_x_object_form_object().is_some() {
            for v in vector_bounds {
                overlap_checks += 1;
                if overlap_checks > MAX_PDF_CONTENT_OPERATIONS {
                    return Err(failure(
                        WorkerFailureCode::InspectionResourceLimitExceeded,
                        "PDF text/vector overlap proof budget exceeded",
                    ));
                }
                if b[0] < v[2] && b[2] > v[0] && b[1] < v[3] && b[3] > v[1] {
                    return Err(failure(
                        WorkerFailureCode::UnsupportedSemanticConstruct,
                        "pdf_vector_text_overlap_unqualified",
                    ));
                }
            }
        }
        painted_bounds.push(b);
    }
    if !clips.is_empty() {
        let mut intersection = [
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::INFINITY,
        ];
        for clip in clips {
            intersection[0] = intersection[0].max(clip[0]);
            intersection[1] = intersection[1].max(clip[1]);
            intersection[2] = intersection[2].min(clip[2]);
            intersection[3] = intersection[3].min(clip[3]);
        }
        for b in &painted_bounds {
            if b[0] < intersection[0]
                || b[1] < intersection[1]
                || b[2] > intersection[2]
                || b[3] > intersection[3]
            {
                return Err(failure(
                    WorkerFailureCode::UnsupportedSemanticConstruct,
                    "pdf_clip_does_not_enclose_paint",
                ));
            }
        }
    }
    Ok(())
}

fn validate_resources(
    document: &Document,
    resources: Option<&lopdf::Dictionary>,
) -> Result<(), WorkerFailure> {
    if let Some(resources) = resources {
        match resources.get_deref(b"ColorSpace", document) {
            Ok(value) => {
                let spaces = value.as_dict().map_err(|_| malformed())?;
                if spaces.has(b"DefaultGray") || spaces.has(b"DefaultRGB") {
                    return Err(unsupported());
                }
            }
            Err(lopdf::Error::DictKey(_)) => {}
            Err(_) => return Err(malformed()),
        }
    }
    Ok(())
}

fn validate_extgstate(
    context: &PdfPaintContext<'_>,
    operation: &lopdf::content::Operation,
    resources: Option<&lopdf::Dictionary>,
) -> Result<(), WorkerFailure> {
    let [Object::Name(name)] = operation.operands.as_slice() else {
        return Err(malformed());
    };
    let states = resources
        .ok_or_else(malformed)?
        .get_deref(b"ExtGState", context.document)
        .and_then(Object::as_dict)
        .map_err(|_| malformed())?;
    let state = states
        .get_deref(name, context.document)
        .and_then(Object::as_dict)
        .map_err(|_| malformed())?;
    for (key, value) in state.iter() {
        match key.as_slice() {
            b"Type" if value.as_name().ok() == Some(b"ExtGState") => {}
            b"BM" if value.as_name().ok() == Some(b"Normal") => {}
            b"ca" | b"CA" => {
                if numeric_values(std::slice::from_ref(value), 1, context.page_number, "alpha")?[0]
                    != 1.0
                {
                    return Err(unsupported());
                }
            }
            b"SMask" if value.as_name().ok() == Some(b"None") => {}
            _ => return Err(unsupported()),
        }
    }
    Ok(())
}

pub(super) fn validate_page_context(
    context: &PdfPaintContext<'_>,
    page: &lopdf::Dictionary,
    resources: Option<&lopdf::Dictionary>,
) -> Result<(), WorkerFailure> {
    if !context.extended_graphics_seen && !context.page_structure_checked {
        return Ok(());
    }
    validate_resources(context.document, resources)?;
    let catalog = context
        .document
        .trailer
        .get_deref(b"Root", context.document)
        .and_then(Object::as_dict)
        .map_err(|_| malformed())?;
    if catalog.has(b"OutputIntents") {
        return Err(unsupported());
    }
    let group = match page.get_deref(b"Group", context.document) {
        Ok(group) => group.as_dict().map_err(|_| malformed())?,
        Err(lopdf::Error::DictKey(_)) => return Ok(()),
        Err(_) => return Err(malformed()),
    };
    // Page transparency groups are qualified only for opaque native text and
    // paths. Images/Form groups need a separate compositing proof.
    if !context.paint_events.is_empty() || context.form_seen {
        return Err(unsupported());
    }
    if group.get(b"S").and_then(Object::as_name).ok() != Some(b"Transparency")
        || group.get(b"CS").and_then(Object::as_name).ok() != Some(b"DeviceRGB")
    {
        return Err(unsupported());
    }
    for (key, value) in group.iter() {
        match key.as_slice() {
            b"Type" if value.as_name().ok() == Some(b"Group") => {}
            b"S" | b"CS" => {}
            b"I" | b"K" if value.as_bool().ok() == Some(false) => {}
            _ => return Err(unsupported()),
        }
    }
    Ok(())
}
