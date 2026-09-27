use std::io::{Cursor, Read, Write};

use document_semantic_inspection_worker::{AdapterProfile, PptxAdapter, SemanticAdapter};
use quick_xml::{Reader, XmlVersion, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pptx/base.pptx");
const DATA_PART: &str = "ppt/diagrams/data1.xml";
const LAYOUT_PART: &str = "ppt/diagrams/layout1.xml";
const GRAPH: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<dgm:dataModel xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <dgm:ptLst>
    <dgm:pt modelId="0" type="doc"/>
    <dgm:pt modelId="1"><dgm:t><a:p><a:r><a:t>Node A</a:t></a:r></a:p></dgm:t></dgm:pt>
    <dgm:pt modelId="2"><dgm:t><a:p><a:r><a:t>Node B</a:t></a:r></a:p></dgm:t></dgm:pt>
  </dgm:ptLst>
  <dgm:cxnLst>
    <dgm:cxn modelId="3" srcId="0" destId="1" srcOrd="0" destOrd="0"/>
    <dgm:cxn modelId="4" srcId="0" destId="2" srcOrd="1" destOrd="0"/>
  </dgm:cxnLst>
  <dgm:bg/><dgm:whole/>
</dgm:dataModel>"#;

const LAYOUT_LITERAL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<dgm:layoutDef xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram">
  <dgm:layoutNode name="diagram">
    <dgm:alg type="lin"><dgm:param type="linDir" val="fromL"/></dgm:alg>
    <dgm:presOf/>
    <dgm:forEach axis="ch" ptType="node">
      <dgm:layoutNode name="node"><dgm:alg type="sp"/><dgm:presOf axis="self"/></dgm:layoutNode>
    </dgm:forEach>
  </dgm:layoutNode>
</dgm:layoutDef>"#;

const LAYOUT_CHARACTER_REFERENCE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<dgm:layoutDef xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram">
  <dgm:layoutNode name="diagram">
    <dgm:alg type="lin"><dgm:param type="linDir" val="from&#76;"/></dgm:alg>
    <dgm:presOf/>
    <dgm:forEach axis="ch" ptType="node">
      <dgm:layoutNode name="node"><dgm:alg type="sp"/><dgm:presOf axis="self"/></dgm:layoutNode>
    </dgm:forEach>
  </dgm:layoutNode>
</dgm:layoutDef>"#;

const QUICK_STYLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<dgm:styleDef xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" uniqueId="urn:dsi:test:smartart-style" minVer="12.0">
  <dgm:title lang="en-US" val="Test style"/><dgm:desc lang="en-US" val=""/>
</dgm:styleDef>"#;

const COLORS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<dgm:colorsDef xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" uniqueId="urn:dsi:test:smartart-colors" minVer="12.0">
  <dgm:title lang="en-US" val="Test colors"/><dgm:desc lang="en-US" val=""/>
</dgm:colorsDef>"#;

#[test]
fn smartart_layout_attribute_character_reference_is_semantic_noise() {
    let baseline = qualified_smartart_package(LAYOUT_LITERAL);
    let mutant = qualified_smartart_package(LAYOUT_CHARACTER_REFERENCE);

    assert_valid_pptx_package(&baseline);
    assert_valid_pptx_package(&mutant);
    assert_smartart_relationships(&baseline);
    assert_smartart_relationships(&mutant);
    assert_only_layout_part_differs(&baseline, &mutant);
    assert_eq!(
        String::from_utf8(read_part(&baseline, DATA_PART)).expect("data part is UTF-8"),
        String::from_utf8(read_part(&mutant, DATA_PART)).expect("mutant data part is UTF-8"),
        "node identities, labels, and connections are unchanged"
    );
    assert_eq!(smartart_layout_direction(&baseline), "fromL");
    assert_eq!(smartart_layout_direction(&mutant), "fromL");

    let profile = AdapterProfile::default();
    let baseline_output = PptxAdapter
        .inspect(&baseline, &profile)
        .expect("qualified literal-attribute SmartArt baseline must be accepted");
    let mutant_output = PptxAdapter
        .inspect(&mutant, &profile)
        .expect("qualified character-reference SmartArt mutant must be accepted");

    assert_eq!(
        baseline_output.semantic_fingerprint(),
        mutant_output.semantic_fingerprint(),
        "XML attribute character references decode to the same SmartArt layout value"
    );
}

fn qualified_smartart_package(layout: &str) -> Vec<u8> {
    let mut parts = read_parts(BASE);
    replace_existing_part(&mut parts, DATA_PART, |_| GRAPH.as_bytes().to_vec());
    replace_existing_part(&mut parts, "ppt/slides/slide1.xml", |part: Vec<u8>| {
        let slide = String::from_utf8(part).expect("slide XML is UTF-8");
        let updated = slide.replace(
            "<dgm:relIds r:dm=\"rIdDiagram\"/>",
            "<dgm:relIds r:dm=\"rIdDiagram\" r:lo=\"rIdLayout\" r:qs=\"rIdQuickStyle\" r:cs=\"rIdColors\"/>",
        );
        assert_ne!(
            slide, updated,
            "SmartArt relIds has all four part references"
        );
        updated.into_bytes()
    });
    replace_existing_part(&mut parts, "ppt/slides/_rels/slide1.xml.rels", |part| {
        let rels = String::from_utf8(part).expect("slide relationships are UTF-8");
        let updated = rels.replace(
            "</Relationships>",
            concat!(
                "<Relationship Id=\"rIdLayout\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/diagramLayout\" Target=\"../diagrams/layout1.xml\"/>",
                "<Relationship Id=\"rIdQuickStyle\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/diagramQuickStyle\" Target=\"../diagrams/quickStyle1.xml\"/>",
                "<Relationship Id=\"rIdColors\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/diagramColors\" Target=\"../diagrams/colors1.xml\"/>",
                "</Relationships>"
            ),
        );
        assert_ne!(rels, updated, "SmartArt part relationships are present");
        updated.into_bytes()
    });
    replace_existing_part(&mut parts, "[Content_Types].xml", |part| {
        let content_types = String::from_utf8(part).expect("content types are UTF-8");
        let updated = content_types.replace(
            "</Types>",
            concat!(
                "<Override PartName=\"/ppt/diagrams/layout1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.drawingml.diagramLayout+xml\"/>",
                "<Override PartName=\"/ppt/diagrams/quickStyle1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.drawingml.diagramStyle+xml\"/>",
                "<Override PartName=\"/ppt/diagrams/colors1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.drawingml.diagramColors+xml\"/>",
                "</Types>"
            ),
        );
        assert_ne!(
            content_types, updated,
            "all SmartArt part content types are declared"
        );
        updated.into_bytes()
    });
    parts.push((LAYOUT_PART.to_owned(), layout.as_bytes().to_vec()));
    parts.push((
        "ppt/diagrams/quickStyle1.xml".to_owned(),
        QUICK_STYLE.as_bytes().to_vec(),
    ));
    parts.push((
        "ppt/diagrams/colors1.xml".to_owned(),
        COLORS.as_bytes().to_vec(),
    ));
    write_parts(parts)
}

fn replace_existing_part(
    parts: &mut [(String, Vec<u8>)],
    name: &str,
    transform: impl FnOnce(Vec<u8>) -> Vec<u8>,
) {
    let (_, contents) = parts
        .iter_mut()
        .find(|(part_name, _)| part_name == name)
        .unwrap_or_else(|| panic!("missing PPTX part {name}"));
    *contents = transform(std::mem::take(contents));
}

fn read_parts(archive: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = ZipArchive::new(Cursor::new(archive)).expect("PPTX is a ZIP archive");
    let mut parts = Vec::with_capacity(zip.len());
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).expect("central-directory entry");
        let name = file.name().to_owned();
        let mut contents = Vec::new();
        file.read_to_end(&mut contents).expect("valid ZIP CRC");
        parts.push((name, contents));
    }
    parts
}

fn read_part(archive: &[u8], name: &str) -> Vec<u8> {
    read_parts(archive)
        .into_iter()
        .find(|(part_name, _)| part_name == name)
        .unwrap_or_else(|| panic!("missing PPTX part {name}"))
        .1
}

fn write_parts(parts: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, contents) in parts {
        writer.start_file(name, options).expect("start PPTX part");
        writer.write_all(&contents).expect("write PPTX part");
    }
    writer.finish().expect("finish PPTX ZIP").into_inner()
}

fn assert_smartart_relationships(archive: &[u8]) {
    let slide =
        String::from_utf8(read_part(archive, "ppt/slides/slide1.xml")).expect("slide XML is UTF-8");
    for relationship in [
        "r:dm=\"rIdDiagram\"",
        "r:lo=\"rIdLayout\"",
        "r:qs=\"rIdQuickStyle\"",
        "r:cs=\"rIdColors\"",
    ] {
        assert!(
            slide.contains(relationship),
            "missing SmartArt reference {relationship}"
        );
    }
    let rels = String::from_utf8(read_part(archive, "ppt/slides/_rels/slide1.xml.rels"))
        .expect("slide relationships are UTF-8");
    for (id, kind, target) in [
        ("rIdDiagram", "diagramData", "../diagrams/data1.xml"),
        ("rIdLayout", "diagramLayout", "../diagrams/layout1.xml"),
        (
            "rIdQuickStyle",
            "diagramQuickStyle",
            "../diagrams/quickStyle1.xml",
        ),
        ("rIdColors", "diagramColors", "../diagrams/colors1.xml"),
    ] {
        let relationship = format!(
            "<Relationship Id=\"{id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}\" Target=\"{target}\"/>"
        );
        assert!(rels.contains(&relationship), "missing {relationship}");
    }
}

fn assert_only_layout_part_differs(baseline: &[u8], mutant: &[u8]) {
    let baseline_parts = read_parts(baseline);
    let mutant_parts = read_parts(mutant);
    assert_eq!(baseline_parts.len(), mutant_parts.len());
    let mut differing_parts = Vec::new();
    for ((baseline_name, baseline_contents), (mutant_name, mutant_contents)) in
        baseline_parts.iter().zip(&mutant_parts)
    {
        assert_eq!(baseline_name, mutant_name, "ZIP entry order is unchanged");
        if baseline_contents != mutant_contents {
            differing_parts.push(baseline_name.as_str());
        }
    }
    assert_eq!(differing_parts, [LAYOUT_PART]);
}

fn assert_valid_pptx_package(bytes: &[u8]) {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).expect("mutant remains a ZIP archive");
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).expect("mutant central-directory entry");
        let name = file.name().to_owned();
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .expect("mutant ZIP CRC is valid");
        if name.ends_with(".xml") || name.ends_with(".rels") {
            let xml = std::str::from_utf8(&contents).expect("mutant XML remains UTF-8");
            let mut reader = Reader::from_str(xml);
            reader.config_mut().check_end_names = true;
            loop {
                match reader.read_event() {
                    Ok(Event::Eof) => break,
                    Ok(_) => {}
                    Err(error) => panic!("mutant part {name} is malformed XML: {error}"),
                }
            }
        }
    }
}

fn smartart_layout_direction(archive: &[u8]) -> String {
    let layout = read_part(archive, LAYOUT_PART);
    let mut reader = Reader::from_reader(layout.as_slice());
    loop {
        match reader.read_event() {
            Ok(Event::Start(event) | Event::Empty(event))
                if event.name().as_ref() == "dgm:param" =>
            {
                let value = event
                    .attributes()
                    .map(|attribute| attribute.expect("valid parameter attribute"))
                    .find(|attribute| attribute.key.as_ref() == "val")
                    .expect("SmartArt layout parameter has val attribute")
                    .normalized_value(XmlVersion::Implicit1_0)
                    .expect("SmartArt layout attribute character reference is valid");
                return value.into_owned();
            }
            Ok(Event::Eof) => panic!("SmartArt layout parameter is present"),
            Ok(_) => {}
            Err(error) => panic!("SmartArt layout XML is valid: {error}"),
        }
    }
}
