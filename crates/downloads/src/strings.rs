use fluxdown_ui_i18n::{Translator, keys};
use gpui::SharedString;

#[derive(Clone)]
pub(crate) struct DownloadStrings {
    pub(crate) category_archive: SharedString,
    pub(crate) category_audio: SharedString,
    pub(crate) category_document: SharedString,
    pub(crate) category_image: SharedString,
    pub(crate) category_other: SharedString,
    pub(crate) category_program: SharedString,
    pub(crate) category_video: SharedString,
    pub(crate) col_created: SharedString,
    pub(crate) col_eta: SharedString,
    pub(crate) col_file_name: SharedString,
    pub(crate) col_size: SharedString,
    pub(crate) col_speed: SharedString,
    pub(crate) col_status: SharedString,
    pub(crate) delete: SharedString,
    pub(crate) later_queue: SharedString,
    pub(crate) new_download: SharedString,
    pub(crate) pause: SharedString,
    pub(crate) resume: SharedString,
    pub(crate) main_queue: SharedString,
    pub(crate) sidebar_queues: SharedString,
    pub(crate) status_all: SharedString,
    pub(crate) status_completed: SharedString,
    pub(crate) status_incomplete: SharedString,
    pub(crate) status_paused: SharedString,
    pub(crate) stop_all: SharedString,
    pub(crate) today: SharedString,
    pub(crate) view_columns_at_least_one: SharedString,
    pub(crate) view_columns_menu_title: SharedString,
    pub(crate) view_columns_reset_action: SharedString,
}

impl DownloadStrings {
    pub(crate) fn from_translator(translator: &Translator) -> Self {
        Self {
            category_archive: shared(translator.text(keys::CATEGORY_ARCHIVE)),
            category_audio: shared(translator.text(keys::CATEGORY_AUDIO)),
            category_document: shared(translator.text(keys::CATEGORY_DOCUMENT)),
            category_image: shared(translator.text(keys::CATEGORY_IMAGE)),
            category_other: shared(translator.text(keys::CATEGORY_OTHER)),
            category_program: shared(translator.text(keys::CATEGORY_PROGRAM)),
            category_video: shared(translator.text(keys::CATEGORY_VIDEO)),
            col_created: shared(translator.text(keys::COL_CREATED)),
            col_eta: shared(translator.text(keys::COL_ETA)),
            col_file_name: shared(translator.text(keys::COL_FILE_NAME)),
            col_size: shared(translator.text(keys::COL_SIZE)),
            col_speed: shared(translator.text(keys::COL_SPEED)),
            col_status: shared(translator.text(keys::COL_STATUS)),
            later_queue: shared(translator.text(keys::LATER_QUEUE)),
            delete: shared(translator.text(keys::DELETE)),
            main_queue: shared(translator.text(keys::MAIN_QUEUE)),
            new_download: shared(translator.text(keys::NEW_DOWNLOAD)),
            pause: shared(translator.text(keys::PAUSE)),
            resume: shared(translator.text(keys::RESUME)),
            sidebar_queues: shared(translator.text(keys::SIDEBAR_QUEUES)),
            status_all: shared(translator.text(keys::TAB_ALL)),
            status_completed: shared(translator.text(keys::STATUS_COMPLETED)),
            status_incomplete: shared(translator.text(keys::STATUS_INCOMPLETE)),
            status_paused: shared(translator.text(keys::STATUS_PAUSED)),
            stop_all: shared(translator.text(keys::STOP_ALL)),
            today: shared(translator.text(keys::TODAY)),
            view_columns_at_least_one: shared(translator.text(keys::VIEW_COLUMNS_AT_LEAST_ONE)),
            view_columns_menu_title: shared(translator.text(keys::VIEW_COLUMNS_MENU_TITLE)),
            view_columns_reset_action: shared(translator.text(keys::VIEW_COLUMNS_RESET_ACTION)),
        }
    }
}

fn shared(value: &str) -> SharedString {
    SharedString::from(value.to_owned())
}
