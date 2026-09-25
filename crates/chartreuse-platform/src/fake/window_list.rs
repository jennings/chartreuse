//! Fake [`WindowList`]: the world's synthetic windows.

use chartreuse_core::window::WindowInfo;
use chartreuse_core::Result;
use futures::future::{self, BoxFuture, FutureExt};

use super::Fake;
use crate::window_list::WindowList;

impl WindowList for Fake {
    fn windows(&self) -> BoxFuture<'static, Result<Vec<WindowInfo>>> {
        let mut windows = self.state.lock().windows.clone();
        windows.sort_by_key(|window| window.z_order);
        future::ready(Ok(windows)).boxed()
    }
}

#[cfg(test)]
mod tests {
    use chartreuse_core::geometry::LogicalRect;
    use chartreuse_core::window::{WindowId, WindowOwner};
    use futures::executor::block_on;

    use super::*;
    use crate::fake::default_displays;

    #[test]
    fn windows_come_back_front_to_back() {
        let window = |id, z_order| WindowInfo {
            id: WindowId(id),
            title: None,
            owner: WindowOwner {
                name: "Test".into(),
                pid: None,
            },
            bounds: LogicalRect::new(0.0, 0.0, 10.0, 10.0),
            z_order,
        };
        let fake = Fake::with_world(
            default_displays(),
            vec![window(1, 2), window(2, 0), window(3, 1)],
        );
        let ids: Vec<u64> = block_on(fake.windows())
            .unwrap()
            .iter()
            .map(|w| w.id.0)
            .collect();
        assert_eq!(ids, [2, 3, 1]);
    }
}
