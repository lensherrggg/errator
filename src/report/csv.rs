use crate::model::Report;

pub const HEADER: &str = "file,rule_id,category,severity,kind,line,column,paragraph,char_offset,original,suggestion,message,source,confidence";

pub fn render(report: &Report) -> String {
    let mut output = String::new();
    output.push_str(HEADER);
    output.push_str("\r\n");
    for issue in &report.issues {
        let file = issue.file.as_deref().unwrap_or(report.source.as_str());
        let (kind, line, column, paragraph, char_offset) = match &issue.location {
            crate::model::Location::Text { line, column, .. } => (
                "text",
                line.to_string(),
                column.to_string(),
                String::new(),
                String::new(),
            ),
            crate::model::Location::Docx {
                paragraph,
                char_offset,
                ..
            } => (
                "docx",
                String::new(),
                String::new(),
                paragraph.to_string(),
                char_offset.to_string(),
            ),
        };
        let row = [
            file.to_owned(),
            issue.rule_id.clone(),
            issue.category.clone(),
            issue.severity.label().to_owned(),
            kind.to_owned(),
            line,
            column,
            paragraph,
            char_offset,
            issue.original.clone(),
            issue.suggestion.clone(),
            issue.message.clone(),
            issue.source.label().to_owned(),
            format!("{:.2}", issue.confidence),
        ];
        output.push_str(
            &row.iter()
                .map(|value| escape(value))
                .collect::<Vec<_>>()
                .join(","),
        );
        output.push_str("\r\n");
    }
    output
}

fn escape(value: &str) -> String {
    if value.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}
