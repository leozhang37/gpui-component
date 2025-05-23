//! Dock is a fixed container that places at left, bottom, right of the Windows.

use std::{ops::Deref, sync::Arc};

use gpui::{
    div, prelude::FluentBuilder as _, px, Along, App, AppContext, Axis, Context, Element, Empty,
    Entity, IntoElement, MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Point, Render,
    Style, StyleRefinement, Styled as _, WeakEntity, Window,
};
use serde::{Deserialize, Serialize};

use crate::{
    resizable::{resize_handle, PANEL_MIN_SIZE},
    StyledExt,
};

use super::{DockArea, DockItem, PanelView, TabPanel};

#[derive(Clone)]
struct ResizePanel;

impl Render for ResizePanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DockPlacement {
    #[serde(rename = "center")]
    Center,
    #[serde(rename = "left")]
    Left,
    #[serde(rename = "bottom")]
    Bottom,
    #[serde(rename = "right")]
    Right,
}

impl DockPlacement {
    fn axis(&self) -> Axis {
        match self {
            Self::Left | Self::Right => Axis::Horizontal,
            Self::Bottom => Axis::Vertical,
            Self::Center => unreachable!(),
        }
    }

    pub fn is_left(&self) -> bool {
        matches!(self, Self::Left)
    }

    pub fn is_bottom(&self) -> bool {
        matches!(self, Self::Bottom)
    }

    pub fn is_right(&self) -> bool {
        matches!(self, Self::Right)
    }
}

/// The Dock is a fixed container that places at left, bottom, right of the Windows.
///
/// This is unlike Panel, it can't be move or add any other panel.
pub struct Dock {
    pub(super) placement: DockPlacement,
    dock_area: WeakEntity<DockArea>,
    pub(crate) panel: DockItem,
    /// The ratio of the dock.
    pub(super) ratio: f32,
    pub(super) open: bool,
    /// Whether the Dock is collapsible, default: true
    pub(super) collapsible: bool,

    // Runtime state
    /// Whether the Dock is resizing
    resizing: bool,
}

impl Dock {
    pub(crate) fn new(
        dock_area: WeakEntity<DockArea>,
        placement: DockPlacement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let panel = cx.new(|cx| {
            let mut tab = TabPanel::new(None, dock_area.clone(), window, cx);
            tab.closable = false;
            tab
        });

        let panel = DockItem::Tabs {
            items: Vec::new(),
            active_ix: 0,
            view: panel.clone(),
        };

        Self::subscribe_panel_events(dock_area.clone(), &panel, window, cx);

        Self {
            placement,
            dock_area,
            panel,
            open: true,
            collapsible: true,
            ratio: 0.2,
            resizing: false,
        }
    }

    pub fn left(
        dock_area: WeakEntity<DockArea>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new(dock_area, DockPlacement::Left, window, cx)
    }

    pub fn bottom(
        dock_area: WeakEntity<DockArea>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new(dock_area, DockPlacement::Bottom, window, cx)
    }

    pub fn right(
        dock_area: WeakEntity<DockArea>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new(dock_area, DockPlacement::Right, window, cx)
    }

    /// Update the Dock to be collapsible or not.
    ///
    /// And if the Dock is not collapsible, it will be open.
    pub fn set_collapsible(&mut self, collapsible: bool, _: &mut Window, cx: &mut Context<Self>) {
        self.collapsible = collapsible;
        if !collapsible {
            self.open = true
        }
        cx.notify();
    }

    pub(super) fn from_state(
        dock_area: WeakEntity<DockArea>,
        placement: DockPlacement,
        ratio: f32,
        panel: DockItem,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::subscribe_panel_events(dock_area.clone(), &panel, window, cx);

        if !open {
            match panel.clone() {
                DockItem::Tabs { view, .. } => {
                    view.update(cx, |panel, cx| {
                        panel.set_collapsed(true, window, cx);
                    });
                }
                DockItem::Split { items, .. } => {
                    for item in items {
                        item.set_collapsed(true, window, cx);
                    }
                }
                _ => {}
            }
        }

        Self {
            placement,
            dock_area,
            panel,
            open,
            ratio,
            collapsible: true,
            resizing: false,
        }
    }

    fn subscribe_panel_events(
        dock_area: WeakEntity<DockArea>,
        panel: &DockItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match panel {
            DockItem::Tabs { view, .. } => {
                window.defer(cx, {
                    let view = view.clone();
                    move |window, cx| {
                        _ = dock_area.update(cx, |this, cx| {
                            this.subscribe_panel(&view, window, cx);
                        });
                    }
                });
            }
            DockItem::Split { items, view, .. } => {
                for item in items {
                    Self::subscribe_panel_events(dock_area.clone(), item, window, cx);
                }
                window.defer(cx, {
                    let view = view.clone();
                    move |window, cx| {
                        _ = dock_area.update(cx, |this, cx| {
                            this.subscribe_panel(&view, window, cx);
                        });
                    }
                });
            }
            DockItem::Tiles { view, .. } => {
                window.defer(cx, {
                    let view = view.clone();
                    move |window, cx| {
                        _ = dock_area.update(cx, |this, cx| {
                            this.subscribe_panel(&view, window, cx);
                        });
                    }
                });
            }
            DockItem::Panel { .. } => {
                // Not supported
            }
        }
    }

    pub fn set_panel(&mut self, panel: DockItem, _: &mut Window, cx: &mut Context<Self>) {
        self.panel = panel;
        cx.notify();
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn toggle_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_open(!self.open, window, cx);
    }

    /// Returns the size of the Dock, the size is means the width or height of
    /// the Dock, if the placement is left or right, the size is width,
    /// otherwise the size is height.
    pub fn ratio(&self) -> f32 {
        self.ratio
    }

    /// Set the size of the Dock.
    pub fn set_ratio(&mut self, ratio: f32, _: &mut Window, cx: &mut Context<Self>) {
        self.ratio = ratio;
        cx.notify();
    }

    /// Set the open state of the Dock.
    pub fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open = open;
        let item = self.panel.clone();
        cx.defer_in(window, move |_, window, cx| {
            item.set_collapsed(!open, window, cx);
        });
        cx.notify();
    }

    /// Add item to the Dock.
    pub fn add_panel(
        &mut self,
        panel: Arc<dyn PanelView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.panel
            .add_panel(panel, &self.dock_area, None, window, cx);
        cx.notify();
    }

    /// Remove item from the Dock.
    pub fn remove_panel(
        &mut self,
        panel: Arc<dyn PanelView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.panel.remove_panel(panel, window, cx);
        cx.notify();
    }

    fn container_size(&self, cx: &App) -> Pixels {
        let dock_area = self
            .dock_area
            .upgrade()
            .expect("DockArea is missing")
            .read(cx);
        let area_bounds = dock_area.bounds;
        area_bounds.size.along(self.placement.axis())
    }

    fn render_resize_handle(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let axis = self.placement.axis();
        let view = cx.entity().clone();

        resize_handle("resize-handle", axis)
            .placement(self.placement)
            .on_drag(ResizePanel {}, move |info, _, _, cx| {
                cx.stop_propagation();
                view.update(cx, |view, _| {
                    view.resizing = true;
                });
                cx.new(|_| info.deref().clone())
            })
    }

    fn resize(&mut self, mouse_position: Point<Pixels>, _: &mut Window, cx: &mut Context<Self>) {
        if !self.resizing {
            return;
        }

        let dock_area = self
            .dock_area
            .upgrade()
            .expect("DockArea is missing")
            .read(cx);
        let area_bounds = dock_area.bounds;

        let size = match self.placement {
            DockPlacement::Left => mouse_position.x - area_bounds.left(),
            DockPlacement::Right => area_bounds.right() - mouse_position.x,
            DockPlacement::Bottom => area_bounds.bottom() - mouse_position.y,
            DockPlacement::Center => unreachable!(),
        };
        let container_size = self.container_size(cx);
        let ratio = size / container_size;
        let min_ratio = PANEL_MIN_SIZE / container_size;
        self.ratio = ratio.clamp(min_ratio, 1.);

        cx.notify();
    }

    fn done_resizing(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.resizing = false;
    }
}

impl Render for Dock {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        if !self.open && !self.placement.is_bottom() {
            return div();
        }

        let cache_style = StyleRefinement::default().absolute().size_full();
        let container_size = self.container_size(cx);
        let dock_size = (self.ratio * container_size).max(PANEL_MIN_SIZE);

        div()
            .relative()
            .overflow_hidden()
            .map(|this| match self.placement {
                DockPlacement::Left | DockPlacement::Right => this.h_flex().h_full().w(dock_size),
                DockPlacement::Bottom => this.w_full().h(dock_size),
                DockPlacement::Center => unreachable!(),
            })
            // Bottom Dock should keep the title bar, then user can click the Toggle button
            .when(!self.open && self.placement.is_bottom(), |this| {
                this.h(px(29.))
            })
            .map(|this| match &self.panel {
                DockItem::Split { view, .. } => this.child(view.clone()),
                DockItem::Tabs { view, .. } => this.child(view.clone()),
                DockItem::Panel { view, .. } => this.child(view.clone().view().cached(cache_style)),
                // Not support to render Tiles and Tile into Dock
                DockItem::Tiles { .. } => this,
            })
            .child(self.render_resize_handle(window, cx))
            .child(DockElement {
                view: cx.entity().clone(),
            })
    }
}

struct DockElement {
    view: Entity<Dock>,
}

impl IntoElement for DockElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for DockElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        window: &mut gpui::Window,
        cx: &mut App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        (window.request_layout(Style::default(), None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: gpui::Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _window: &mut gpui::Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
        ()
    }

    fn paint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: gpui::Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut gpui::Window,
        cx: &mut App,
    ) {
        window.on_mouse_event({
            let view = self.view.clone();
            let resizing = view.read(cx).resizing;
            move |e: &MouseMoveEvent, phase, window, cx| {
                if !resizing {
                    return;
                }
                if !phase.bubble() {
                    return;
                }

                view.update(cx, |view, cx| view.resize(e.position, window, cx))
            }
        });

        // When any mouse up, stop dragging
        window.on_mouse_event({
            let view = self.view.clone();
            move |_: &MouseUpEvent, phase, window, cx| {
                if phase.bubble() {
                    view.update(cx, |view, cx| view.done_resizing(window, cx));
                }
            }
        })
    }
}
