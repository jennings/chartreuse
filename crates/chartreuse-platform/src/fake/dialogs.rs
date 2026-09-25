//! Fake [`FileDialogs`]: scripted answers, recorded requests.

use std::path::PathBuf;

use chartreuse_core::Result;
use futures::future::{self, BoxFuture, FutureExt};

use super::Fake;
use crate::dialogs::{FileDialogs, OpenImageRequest, SaveImageRequest};

impl FileDialogs for Fake {
    fn open_image(
        &self,
        _request: OpenImageRequest,
    ) -> BoxFuture<'static, Result<Option<PathBuf>>> {
        future::ready(Ok(self.state.lock().open_answer.clone())).boxed()
    }

    fn save_image(&self, request: SaveImageRequest) -> BoxFuture<'static, Result<Option<PathBuf>>> {
        let mut state = self.state.lock();
        state.save_requests.push(request);
        future::ready(Ok(state.save_answer.clone())).boxed()
    }
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;

    use super::*;

    #[test]
    fn dialogs_return_the_scripted_answer_and_record_save_requests() {
        let fake = Fake::new();
        let open = OpenImageRequest {
            title: "Open".into(),
            extensions: vec!["png".into()],
        };
        assert_eq!(
            block_on(fake.open_image(open.clone())).unwrap(),
            None,
            "cancelled by default"
        );
        fake.set_open_answer(Some("/tmp/in.png".into()));
        assert_eq!(
            block_on(fake.open_image(open)).unwrap(),
            Some("/tmp/in.png".into())
        );

        let save = SaveImageRequest {
            title: "Save".into(),
            directory: None,
            file_name: "shot.png".into(),
            extensions: vec!["png".into()],
        };
        fake.set_save_answer(Some("/tmp/out.png".into()));
        assert_eq!(
            block_on(fake.save_image(save.clone())).unwrap(),
            Some("/tmp/out.png".into())
        );
        assert_eq!(fake.save_requests(), [save]);
    }
}
