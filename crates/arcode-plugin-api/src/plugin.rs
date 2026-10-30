//! The [`Plugin`] trait and the `export_plugin!` macro.
//!
//! WIT exports are free functions; the [`Adapter`] keeps one plugin value in
//! a thread-local (guest code is single-threaded) and forwards every export
//! to it. Every method has a default, so a plugin implements only what it
//! contributes and a new export never breaks existing source (§11).

use std::any::Any;
use std::cell::RefCell;
use std::marker::PhantomData;

use crate::args::{Error, Json, Result};
use crate::bindings::arcode::plugin::editor::DocInfo;
use crate::bindings::arcode::plugin::types::{NodeId, PanelId};
use crate::bindings::arcode::plugin::ui::{Event, RowPage};
use crate::bindings::exports::arcode::plugin::guest::{Activation, CodeLens, Guest};

/// A plugin. Implement the methods for what the manifest contributes and
/// register the type with [`export_plugin!`](crate::export_plugin).
pub trait Plugin: Default + 'static {
    /// First call after the instance is created. `reason` says which
    /// contribution triggered it, or `Restore` after an idle unload or crash.
    fn activate(&mut self, reason: Activation) -> Result<()> {
        let _ = reason;
        Ok(())
    }

    /// Last call before the instance is dropped (100 ms budget).
    fn deactivate(&mut self) {}

    /// A contributed command ran. `id` is the manifest-local command id.
    fn command(&mut self, id: &str, args: Json) -> Result<()> {
        let _ = args;
        Err(Error::msg(format!("unknown command `{id}`")))
    }

    /// Code lenses for a visible document of a contributed lens language.
    fn code_lenses(&mut self, doc: &DocInfo) -> Vec<CodeLens> {
        let _ = doc;
        Vec::new()
    }

    /// A contributed panel or opener was shown in `panel`.
    fn open_view(&mut self, panel: PanelId, contribution: &str, args: Json) -> Result<()> {
        let _ = (panel, args);
        Err(Error::msg(format!("no view for `{contribution}`")))
    }

    /// A widget event from one of this plugin's panels.
    fn ui_event(&mut self, panel: PanelId, event: Event) -> Result<()> {
        let _ = (panel, event);
        Ok(())
    }

    /// The host wants rows `start..start + count` of a `table` node.
    fn fetch_rows(
        &mut self,
        panel: PanelId,
        table: NodeId,
        start: u64,
        count: u32,
    ) -> Result<RowPage> {
        let _ = (panel, table, count);
        Ok(RowPage {
            start,
            rows: Vec::new(),
            at_end: true,
        })
    }

    /// The user closed `panel`.
    fn view_closed(&mut self, panel: PanelId) {
        let _ = panel;
    }
}

thread_local! {
    /// The plugin value, created on first use. `Box<dyn Any>` because a
    /// static cannot be generic over the plugin type.
    static INSTANCE: RefCell<Option<Box<dyn Any>>> = const { RefCell::new(None) };
}

/// Runs `f` with the plugin instance, creating it on first use.
///
/// The component model forbids re-entering a guest while it is inside a
/// host import, so the borrow cannot be held twice; if it ever were, the
/// call fails with an error instead of panicking.
pub(crate) fn with_plugin<T: Plugin, R>(f: impl FnOnce(&mut T) -> R) -> Result<R> {
    INSTANCE.with(|cell| {
        let mut slot = cell
            .try_borrow_mut()
            .map_err(|_| Error::msg("plugin re-entered while busy"))?;
        let any = slot.get_or_insert_with(|| Box::new(T::default()));
        match any.downcast_mut::<T>() {
            Some(plugin) => Ok(f(plugin)),
            None => Err(Error::msg("plugin instance has an unexpected type")),
        }
    })
}

/// Drops the plugin instance (after `deactivate`).
fn drop_plugin() {
    INSTANCE.with(|cell| {
        if let Ok(mut slot) = cell.try_borrow_mut() {
            *slot = None;
        }
    });
}

/// Bridges the generated `Guest` trait to a [`Plugin`]. Used by
/// [`export_plugin!`](crate::export_plugin); not meant to be named directly.
#[doc(hidden)]
pub struct Adapter<T>(PhantomData<T>);

fn flatten<T>(result: Result<Result<T>>) -> std::result::Result<T, String> {
    result.and_then(|inner| inner).map_err(Error::into_message)
}

impl<T: Plugin> Guest for Adapter<T> {
    fn activate(reason: Activation) -> std::result::Result<(), String> {
        flatten(with_plugin::<T, _>(|p| p.activate(reason)))
    }

    fn deactivate() {
        let _ = with_plugin::<T, _>(|p| p.deactivate());
        drop_plugin();
    }

    fn handle_command(command: String, args: String) -> std::result::Result<(), String> {
        flatten(with_plugin::<T, _>(|p| {
            p.command(&command, Json::from(args))
        }))
    }

    fn provide_code_lenses(doc: DocInfo) -> Vec<CodeLens> {
        with_plugin::<T, _>(|p| p.code_lenses(&doc)).unwrap_or_default()
    }

    fn open_view(
        panel: PanelId,
        contribution: String,
        args: String,
    ) -> std::result::Result<(), String> {
        flatten(with_plugin::<T, _>(|p| {
            p.open_view(panel, &contribution, Json::from(args))
        }))
    }

    fn handle_ui_event(panel: PanelId, event: Event) -> std::result::Result<(), String> {
        flatten(with_plugin::<T, _>(|p| p.ui_event(panel, event)))
    }

    fn fetch_rows(
        panel: PanelId,
        table: NodeId,
        start: u64,
        count: u32,
    ) -> std::result::Result<RowPage, String> {
        flatten(with_plugin::<T, _>(|p| {
            p.fetch_rows(panel, table, start, count)
        }))
    }

    fn view_closed(panel: PanelId) {
        let _ = with_plugin::<T, _>(|p| p.view_closed(panel));
    }
}

/// Exports a [`Plugin`] type as the component's `arcode:plugin/guest`.
///
/// ```ignore
/// #[derive(Default)]
/// struct Echo;
/// impl arcode_plugin::Plugin for Echo {}
/// arcode_plugin::export_plugin!(Echo);
/// ```
#[macro_export]
macro_rules! export_plugin {
    ($plugin:ty) => {
        const _: () = {
            type __ArcodePluginAdapter = $crate::__private::Adapter<$plugin>;
    $crate::__export_plugin_impl!(__ArcodePluginAdapter with_types_in $crate::bindings);
        };
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Counter {
        commands: u32,
    }

    impl Plugin for Counter {
        fn command(&mut self, id: &str, _args: Json) -> Result<()> {
            self.commands += 1;
            if id == "fail" {
                Err(Error::msg("boom"))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn adapter_keeps_state_between_calls_and_drops_it_on_deactivate() {
        drop_plugin();
        assert_eq!(
            <Adapter<Counter> as Guest>::handle_command("a".into(), "null".into()),
            Ok(())
        );
        assert_eq!(
            <Adapter<Counter> as Guest>::handle_command("fail".into(), "".into()),
            Err("boom".into())
        );
        assert_eq!(with_plugin::<Counter, _>(|c| c.commands), Ok(2));
        <Adapter<Counter> as Guest>::deactivate();
        assert_eq!(with_plugin::<Counter, _>(|c| c.commands), Ok(0));
    }

    #[test]
    fn defaults_are_harmless() {
        #[derive(Default)]
        struct Empty;
        impl Plugin for Empty {}
        drop_plugin();
        assert!(<Adapter<Empty> as Guest>::activate(Activation::Restore).is_ok());
        assert!(<Adapter<Empty> as Guest>::handle_command("x".into(), "".into()).is_err());
        let page = <Adapter<Empty> as Guest>::fetch_rows(1, 2, 40, 10);
        assert_eq!(
            page,
            Ok(RowPage {
                start: 40,
                rows: vec![],
                at_end: true
            })
        );
        drop_plugin();
    }
}
