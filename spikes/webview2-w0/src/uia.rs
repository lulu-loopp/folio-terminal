//! Gate 6: can a screen reader read the page?
//!
//! Two questions, and they are not the same one. The engine exposes an
//! `AutomationProvider` on the composition controller — that is the *fragment*
//! a host is expected to graft into its own provider tree. Whether Narrator can
//! actually reach the page is a different question, answered by walking the
//! window's real UI Automation tree from the outside, exactly as an assistive
//! technology does.

use anyhow::{Context as _, Result};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, TreeScope_Children,
};

#[derive(Debug, serde::Serialize)]
pub struct Node {
    pub depth: u32,
    pub name: String,
    pub class: String,
    pub control_type: i32,
    pub framework: String,
}

/// A Chromium document is thousands of nodes deep and wide. The gate is asking
/// whether the page is *reachable*, not cataloguing it, so the walk stops once
/// it has seen enough to answer.
const NODE_BUDGET: usize = 600;

/// Walk the automation tree under `hwnd` and report the nodes found, down to
/// `max_depth` and up to [`NODE_BUDGET`].
pub fn walk(hwnd: HWND, max_depth: u32) -> Result<Vec<Node>> {
    unsafe {
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                .context("CoCreateInstance(CUIAutomation)")?;
        let root = automation
            .ElementFromHandle(hwnd)
            .context("ElementFromHandle")?;
        let mut nodes = Vec::new();
        descend(&automation, &root, 0, max_depth, &mut nodes)?;
        Ok(nodes)
    }
}

fn descend(
    automation: &IUIAutomation,
    element: &IUIAutomationElement,
    depth: u32,
    max_depth: u32,
    nodes: &mut Vec<Node>,
) -> Result<()> {
    if nodes.len() >= NODE_BUDGET {
        return Ok(());
    }
    unsafe {
        nodes.push(Node {
            depth,
            name: element
                .CurrentName()
                .map(|name| name.to_string())
                .unwrap_or_default(),
            class: element
                .CurrentClassName()
                .map(|name| name.to_string())
                .unwrap_or_default(),
            control_type: element.CurrentControlType().map(|kind| kind.0).unwrap_or(0),
            framework: element
                .CurrentFrameworkId()
                .map(|id| id.to_string())
                .unwrap_or_default(),
        });
        if depth >= max_depth {
            return Ok(());
        }
        // `TrueCondition` rather than a filter: gate 6 is asking what is
        // reachable at all, and a filtered walk could hide the very node whose
        // absence is the finding.
        let condition = automation
            .CreateTrueCondition()
            .context("CreateTrueCondition")?;
        let children = element
            .FindAll(TreeScope_Children, &condition)
            .context("FindAll")?;
        let count = children.Length().unwrap_or(0);
        for index in 0..count {
            if let Ok(child) = children.GetElement(index) {
                descend(automation, &child, depth + 1, max_depth, nodes)?;
            }
        }
        Ok(())
    }
}

/// Whether any node in the walk carries a name containing `needle`.
pub fn contains_name(nodes: &[Node], needle: &str) -> bool {
    nodes
        .iter()
        .any(|node| node.name.to_lowercase().contains(&needle.to_lowercase()))
}

/// The frameworks seen in the walk — `Win32` for the host, `Chrome` (or `Edge`)
/// for anything the engine put there.
pub fn frameworks(nodes: &[Node]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for node in nodes {
        if !node.framework.is_empty() && !seen.contains(&node.framework) {
            seen.push(node.framework.clone());
        }
    }
    seen
}

/// Control types seen, for the report's "did a document node appear" line.
/// `UIA_DocumentControlTypeId` is 50030.
pub fn has_document(nodes: &[Node]) -> bool {
    nodes.iter().any(|node| node.control_type == 50030)
}

/// The `AutomationProvider` the composition controller offers. Reading it is the
/// first half of gate 6; whether it is *reachable* is the second.
pub fn provider_present(
    composition: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2CompositionController,
) -> Result<bool> {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2CompositionController2;
    use windows::core::Interface as _;
    let controller2: ICoreWebView2CompositionController2 = composition
        .cast()
        .context("ICoreWebView2CompositionController2")?;
    let provider = unsafe { controller2.AutomationProvider() }.context("AutomationProvider")?;
    Ok(!provider.as_raw().is_null())
}
