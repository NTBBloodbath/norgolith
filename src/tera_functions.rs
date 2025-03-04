use std::collections::HashMap;

use eyre::Result;
use tera::{Error, Function, Value};

/// Now function
/// Template usage: {{ now(format="%A, %B %d") }} → "Thursday, October 05"
pub struct NowFunction;
impl Function for NowFunction {
    fn call(&self, args: &HashMap<String, Value>) -> Result<Value, Error> {
        let format = match args.get("format") {
            Some(v) => v
                .as_str()
                .ok_or(tera::Error::msg("`format` must be a string"))?,
            None => "%Y-%m-%d %H:%M:%S", // Default format
        };

        let now = chrono::Local::now();
        Ok(Value::String(now.format(format).to_string()))
    }
}

#[derive(Debug, Clone)]
struct TocNode {
    level: u8,
    title: String,
    id: String,
    children: Vec<usize>,
}

#[derive(Debug)]
struct TocTree {
    nodes: Vec<TocNode>,
    root_indices: Vec<usize>,
}

fn parse_toc(value: &Value) -> Result<TocTree> {
    let entries = value.as_array().ok_or("TOC must be an array").unwrap();
    let mut tree = TocTree {
        nodes: Vec::new(),
        root_indices: Vec::new(),
    };
    let mut stack: Vec<usize> = Vec::new();  // Store indices instead of references

    for entry in entries {
        let level = entry.get("level")
            .and_then(|v| v.as_i64())
            .ok_or("Missing or invalid level").unwrap() as u8;

        let title = entry.get("title")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        let id = entry.get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        // Find the parent index
        let parent_idx = stack.iter().rev().find(|&&idx| {
            tree.nodes[idx].level < level
        }).copied();

        // Create new node
        let node_idx = tree.nodes.len();
        tree.nodes.push(TocNode {
            level,
            title,
            id,
            children: Vec::new(),
        });

        // Add to parent or root
        if let Some(parent_idx) = parent_idx {
            tree.nodes[parent_idx].children.push(node_idx);
        } else {
            tree.root_indices.push(node_idx);
        }

        // Update stack
        while stack.last().map(|&idx| tree.nodes[idx].level >= level).unwrap_or(false) {
            stack.pop();
        }
        stack.push(node_idx);
    }

    Ok(tree)
}

fn generate_nested_html(tree: &TocTree, list_type: &str) -> String {
    fn render_node(tree: &TocTree, node_idx: usize, list_type: &str) -> String {
        let node = &tree.nodes[node_idx];

        let mut html = format!("<li><a href=\"#{}\">{}</a>", node.id, node.title);

        if !node.children.is_empty() {
            html.push_str(&format!("<{}>", list_type));
            for &child_idx in &node.children {
                html.push_str(&render_node(tree, child_idx, list_type));
            }
            html.push_str(&format!("</{}>", list_type));
        }

        html.push_str("</li>");
        html
    }

    let mut html = format!("<{}>", list_type);
    for &root_idx in &tree.root_indices {
        html.push_str(&render_node(tree, root_idx, list_type));
    }
    html.push_str(&format!("</{}>", list_type));
    html
}

pub struct GenerateToc;
impl Function for GenerateToc {
    fn call(&self, args: &HashMap<String, Value>) -> Result<Value, Error> {
        let toc = args.get("toc").expect("Missing 'toc' argument");
        let list_type = args.get("list_type")
            .and_then(|v| v.as_str())
            .unwrap_or("ol");

        let nodes = parse_toc(toc).unwrap();
        let html = generate_nested_html(&nodes, list_type);
        Ok(Value::String(html))
    }

    fn is_safe(&self) -> bool {
        true
    }
}
