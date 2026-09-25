use std::collections::BTreeMap;

pub fn minimal_text_pdf(text: &str, producer: &str) -> Vec<u8> {
    let escaped = text.replace('\\', "\\\\").replace('(', "\\(").replace(')', "\\)");
    let producer = producer.replace('\\', "\\\\").replace('(', "\\(").replace(')', "\\)");
    let content = format!("BT /F1 12 Tf 20 160 Td ({escaped}) Tj ET");
    let mut objects = BTreeMap::<u32, Vec<u8>>::new();
    objects.insert(1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.insert(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec());
    objects.insert(3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec());
    objects.insert(4, format!("<< /Length {} >>\nstream\n{}\nendstream", content.len(), content).into_bytes());
    objects.insert(5, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec());
    objects.insert(6, format!("<< /Producer ({producer}) >>").into_bytes());
    write_pdf(objects, 1, Some(6))
}

pub fn ambiguous_text_pdf() -> Vec<u8> {
    let content = concat!(
        "BT /F1 12 Tf 20 160 Td (Alpha) Tj ET\n",
        "BT /F1 12 Tf 20 160 Td (Beta) Tj ET"
    );
    let mut objects = BTreeMap::<u32, Vec<u8>>::new();
    objects.insert(1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.insert(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec());
    objects.insert(3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec());
    objects.insert(4, format!("<< /Length {} >>\nstream\n{}\nendstream", content.len(), content).into_bytes());
    objects.insert(5, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec());
    objects.insert(6, b"<< /Producer (DSI PoC) >>".to_vec());
    write_pdf(objects, 1, Some(6))
}

pub fn with_broken_startxref(mut input: Vec<u8>) -> Vec<u8> {
    let marker=b"startxref\n";let p=input.windows(marker.len()).rposition(|w|w==marker).expect("startxref");
    let s=p+marker.len();let e=input[s..].iter().position(|b|*b==b'\n').map(|n|s+n).expect("newline");
    input.splice(s..e,b"1".iter().copied());input
}
fn write_pdf(objects:BTreeMap<u32,Vec<u8>>,root:u32,info:Option<u32>)->Vec<u8>{
    let mut bytes=b"%PDF-1.7\n%DSI-PoC\n".to_vec();let mut offsets=BTreeMap::new();
    for(id,obj)in &objects{offsets.insert(*id,bytes.len());bytes.extend_from_slice(format!("{id} 0 obj\n").as_bytes());bytes.extend_from_slice(obj);bytes.extend_from_slice(b"\nendobj\n");}
    let xref=bytes.len();let max=*objects.keys().max().expect("objects");
    bytes.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n",max+1).as_bytes());
    for id in 1..=max{match offsets.get(&id){Some(o)=>bytes.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes()),None=>bytes.extend_from_slice(b"0000000000 65535 f \n")}}
    let mut trailer=format!("trailer\n<< /Size {} /Root {root} 0 R",max+1);if let Some(i)=info{trailer.push_str(&format!(" /Info {i} 0 R"));}trailer.push_str(&format!(" >>\nstartxref\n{xref}\n%%EOF\n"));bytes.extend_from_slice(trailer.as_bytes());bytes
}
