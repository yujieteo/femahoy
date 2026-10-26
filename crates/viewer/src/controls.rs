//! The camera toolbar of the shared UI layer. Both page types get this component from the
//! viewer, so its native HTML controls have the same keyboard paths and VoiceOver labels.

use eframe::egui;
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use web_sys::{Element, HtmlCanvasElement, HtmlElement, HtmlSelectElement, KeyboardEvent};

use crate::{app::RequestSlot, view::Control};

const TOOLBAR: &str = r#"
<div class="toolbar" role="group" aria-label="Camera controls">
  <button type="button" data-camera="fit" aria-keyshortcuts="F" title="Fit the full model. Keep the camera direction.">Fit <kbd>F</kbd></button>
  <button type="button" data-camera="report-view" aria-keyshortcuts="Home" title="Restore the view of the report">Report view <kbd>Home</kbd></button>
  <label data-camera="views">Views <select id="fh-direction" aria-label="Camera direction">
    <option disabled>Custom</option><option>+X</option><option>-X</option><option>+Y</option><option>-Y</option><option>+Z</option><option>-Z</option><option>Isometric</option>
  </select></label>
  <button type="button" data-camera="help" id="fh-help-button" aria-controls="fh-camera-help" aria-expanded="false" aria-keyshortcuts="?">Help <kbd>?</kbd></button>
</div>
<div class="toolbar" id="fh-camera-help" data-camera-help hidden><p class="keys">
  <span><kbd>Drag</kbd> Orbit</span><span><kbd>Right or Shift drag</kbd> Pan</span>
  <span><kbd>Scroll</kbd> Zoom</span><span><kbd>Double click</kbd> Report view</span>
  <span><kbd>F</kbd> Fit</span><span><kbd>Home</kbd> Report view</span><span><kbd>?</kbd> Help</span>
  <span>Shortcuts act when the model has focus.</span>
</p></div>"#;

fn listen(
    element: &Element,
    event: &str,
    action: impl FnMut(web_sys::Event) + 'static,
) -> Result<(), JsValue> {
    let callback = Closure::wrap(Box::new(action) as Box<dyn FnMut(web_sys::Event)>);
    element.add_event_listener_with_callback(event, callback.as_ref().unchecked_ref())?;
    // The toolbar and its listeners live for the lifetime of this viewer page.
    callback.forget();
    Ok(())
}

fn toggle_help(toolbar: &Element) {
    let (Ok(Some(button)), Ok(Some(row))) = (
        toolbar.query_selector("[data-camera='help']"),
        toolbar.query_selector("[data-camera-help]"),
    ) else {
        return;
    };
    let open = button.get_attribute("aria-expanded").as_deref() != Some("true");
    let _ = button.set_attribute("aria-expanded", if open { "true" } else { "false" });
    if open {
        let _ = row.remove_attribute("hidden");
    } else {
        let _ = row.set_attribute("hidden", "");
    }
}

pub fn install(
    canvas: &HtmlCanvasElement,
    slot: RequestSlot,
    ctx: egui::Context,
) -> Result<(), JsValue> {
    let doc = canvas.owner_document().ok_or("no document")?;
    let view = canvas.parent_element().ok_or("no model area")?;
    let model = view.parent_element().ok_or("no model view")?;
    let toolbar = doc.create_element("section")?;
    toolbar.set_id(&format!("{}-camera-controls", canvas.id()));
    toolbar.set_attribute("aria-label", "Camera controls")?;
    toolbar.set_attribute("hidden", "")?;
    toolbar.set_inner_html(TOOLBAR);
    // Each canvas owns its toolbar, also when a page compares two runs.
    for id in ["fh-direction", "fh-help-button", "fh-camera-help"] {
        if let Some(element) = toolbar.query_selector(&format!("#{id}"))? {
            element.set_id(&format!("{}-{id}", canvas.id()));
        }
    }
    if let Some(button) = toolbar.query_selector("[data-camera='help']")? {
        button.set_attribute("aria-controls", &format!("{}-fh-camera-help", canvas.id()))?;
    }
    model.insert_before(&toolbar, Some(&view))?;
    for name in ["fit", "report-view", "views", "help"] {
        let control = toolbar
            .query_selector(&format!("[data-camera='{name}']"))?
            .ok_or("no camera control")?;
        let event = if name == "views" { "change" } else { "click" };
        let (slot, ctx, canvas) = (slot.clone(), ctx.clone(), canvas.clone());
        let toolbar = toolbar.clone();
        listen(&control, event, move |event| {
            if name == "help" {
                toggle_help(&toolbar);
                return;
            }
            let action = if name == "views" {
                let Some(select) = event
                    .target()
                    .and_then(|t| t.dyn_into::<HtmlSelectElement>().ok())
                else {
                    return;
                };
                select.value()
            } else {
                name.to_owned()
            };
            if let Ok(mut r) = slot.lock() {
                r.camera = Some(action);
            }
            ctx.request_repaint();
            // Keep a native select focused so arrow keys can change its value.
            if name != "views" {
                let _ = canvas.focus();
            }
        })?;
    }
    let element: &Element = canvas.as_ref();
    let target = canvas.clone();
    listen(element, "keydown", move |event| {
        let Some(key) = event.dyn_ref::<KeyboardEvent>() else {
            return;
        };
        if key.ctrl_key()
            || key.alt_key()
            || key.meta_key()
            || key.target().as_ref() != Some(target.as_ref())
        {
            return;
        }
        let action = match key.key().as_str() {
            "f" | "F" => "fit",
            "Home" => "report-view",
            "?" => {
                toggle_help(&toolbar);
                key.prevent_default();
                return;
            }
            _ => return,
        };
        if let Ok(mut r) = slot.lock() {
            r.camera = Some(action.into());
        }
        ctx.request_repaint();
        key.prevent_default();
    })?;
    Ok(())
}

/// Show only the named controls, unless the person asks for all controls.
pub fn update(canvas_id: &str, controls: &[Control], all: bool, direction: &str) {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Some(toolbar) = doc.get_element_by_id(&format!("{canvas_id}-camera-controls")) else {
        return;
    };
    let mut any = false;
    for control in [
        Control::Fit,
        Control::ReportView,
        Control::Views,
        Control::Help,
    ] {
        let visible = all || controls.contains(&control);
        any |= visible;
        if let Ok(Some(element)) =
            toolbar.query_selector(&format!("[data-camera='{}']", control.name()))
        {
            element.unchecked_ref::<HtmlElement>().set_hidden(!visible);
        }
        if control == Control::Help && !visible {
            if let Ok(Some(row)) = toolbar.query_selector("[data-camera-help]") {
                let _ = row.set_attribute("hidden", "");
            }
            if let Ok(Some(button)) = toolbar.query_selector("[data-camera='help']") {
                let _ = button.set_attribute("aria-expanded", "false");
            }
        }
    }
    toolbar.unchecked_ref::<HtmlElement>().set_hidden(!any);
    // Preserve a native menu choice until its change event commits the new direction.
    if toolbar.get_attribute("data-direction").as_deref() != Some(direction) {
        let _ = toolbar.set_attribute("data-direction", direction);
        if let Ok(Some(select)) = toolbar.query_selector("select") {
            select
                .unchecked_ref::<HtmlSelectElement>()
                .set_value(direction);
        }
    }
}
