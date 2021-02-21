use gio::prelude::*;

use glib::clone;

use gtk::prelude::*;

use osmgpsmap::{
    MapExt, MapPoint, MapPolygon, MapPolygonExt, MapTrackExt,
};

use roadtrip::core::geometry::Filter;
use roadtrip::core::media::{Media, Thumbnails};
use roadtrip::core::Hash;
use roadtrip::ingest::error::Error as IngestError;
use roadtrip::viewer::{Event, SyncHandle};

use std::cell::RefCell;
use std::collections::HashMap;
use std::convert::TryInto;
use std::rc::{Rc, Weak};

const ICON: &[u8] = include_bytes!("../assets/icon.gdk");
const PLACEHOLDER: &[u8] = include_bytes!("../assets/placeholder.gdk");

#[derive(Debug)]
struct DatePicker {
    label: gtk::Label,
    vbox: gtk::Box,
    hbox: gtk::Box,
    switch: gtk::Switch,
    calendar: gtk::Calendar,
}

impl DatePicker {
    pub fn new(label: &str) -> Self {
        Self {
            label: gtk::Label::new(Some(label)),
            hbox: gtk::Box::new(gtk::Orientation::Horizontal, 3),
            vbox: gtk::Box::new(gtk::Orientation::Vertical, 12),
            calendar: gtk::Calendar::new(),
            switch: gtk::Switch::new(),
        }
    }

    pub fn build(&self) {
        self.calendar.set_sensitive(false);
        self.calendar.select_day(0);

        self.label.set_halign(gtk::Align::Start);

        self.switch.set_active(false);
        self.switch.set_halign(gtk::Align::End);
        self.switch.connect_property_active_notify(
            clone!(@weak self.calendar as calendar => move |switch| {
                let active = switch.get_active();
                calendar.set_sensitive(active);
                if active {
                    // TODO: Maybe save the previous value somewhere.
                    calendar.select_day(1);
                } else {
                    calendar.select_day(0);
                }
            }),
        );

        self.hbox.pack_start(&self.label, true, true, 0);
        self.hbox.add(&self.switch);

        self.vbox.pack_start(&self.hbox, false, false, 0);
        self.vbox.pack_end(&self.calendar, true, true, 0);
    }

    pub fn get_date(&self) -> Option<glib::Date> {
        if !self.switch.get_active() {
            return None;
        }

        let (year, month, day) = self.calendar.get_date();

        let gday: u8 = day.try_into().expect("day out of range");
        let gyear: u16 = year.try_into().expect("year out of range");
        let gmonth = match month {
            0 => glib::DateMonth::January,
            1 => glib::DateMonth::February,
            2 => glib::DateMonth::March,
            3 => glib::DateMonth::April,
            4 => glib::DateMonth::May,
            5 => glib::DateMonth::June,
            6 => glib::DateMonth::July,
            7 => glib::DateMonth::August,
            8 => glib::DateMonth::September,
            9 => glib::DateMonth::October,
            10 => glib::DateMonth::November,
            11 => glib::DateMonth::December,
            _ => glib::DateMonth::BadMonth,
        };

        Some(glib::Date::new_dmy(gday, gmonth, gyear))
    }
}

#[derive(Debug)]
struct FilterMenu {
    btn: gtk::MenuButton,
    img: gtk::Image,
    pop: gtk::Popover,
    dates_box: gtk::Box,
    hide_after: DatePicker,
    hide_before: DatePicker,
}

impl FilterMenu {
    pub fn new() -> Self {
        let btn = gtk::MenuButton::new();
        Self {
            img: gtk::Image::new(),
            pop: gtk::Popover::new(Some(&btn)),
            dates_box: gtk::Box::new(gtk::Orientation::Horizontal, 10),
            hide_after: DatePicker::new("Hide After"),
            hide_before: DatePicker::new("Hide Before"),
            btn,
        }
    }

    pub fn build(&self) {
        self.hide_before.build();
        self.hide_after.build();

        self.img
            .set_from_icon_name(Some("system-search"), gtk::IconSize::Button);

        self.btn.set_image(Some(&self.img));
        self.btn.set_popover(Some(&self.pop));

        self.dates_box.add(&self.hide_before.vbox);
        self.dates_box
            .add(&gtk::Separator::new(gtk::Orientation::Vertical));
        self.dates_box.add(&self.hide_after.vbox);

        self.pop.add(&self.dates_box);
        self.dates_box.show_all();
    }
}

#[derive(Debug, Clone)]
struct MainMenu {
    btn: gtk::MenuButton,
    img: gtk::Image,
    pop: gtk::Popover,
    menu: gio::Menu,
    app_menu: gio::Menu,
}

impl MainMenu {
    pub fn new() -> Self {
        let btn = gtk::MenuButton::new();

        Self {
            menu: gio::Menu::new(),
            app_menu: gio::Menu::new(),
            img: gtk::Image::new(),
            pop: gtk::Popover::new(Some(&btn)),
            btn,
        }
    }

    pub fn build(&self) {
        self.app_menu.append(Some("About"), Some("app.about"));
        self.app_menu.freeze();

        self.menu.append_section(None, &self.app_menu);
        self.menu.freeze();

        self.pop.bind_model(Some(&self.menu), None);

        self.img
            .set_from_icon_name(Some("open-menu"), gtk::IconSize::Button);

        self.btn.set_image(Some(&self.img));
        self.btn.set_popover(Some(&self.pop));
    }
}

#[derive(Debug, Clone)]
pub struct Main(Rc<Inner>);

#[derive(Debug, Clone)]
pub struct MainWeak(Weak<Inner>);

impl glib::clone::Downgrade for Main {
    type Weak = MainWeak;

    fn downgrade(&self) -> MainWeak {
        MainWeak(self.0.downgrade())
    }
}

impl glib::clone::Upgrade for MainWeak {
    type Strong = Main;

    fn upgrade(&self) -> Option<Main> {
        self.0.upgrade().map(Main)
    }
}

#[derive(Debug)]
struct Inner {
    viewer: RefCell<SyncHandle>,
    application: gtk::Application,
    window: gtk::ApplicationWindow,
    header_bar: gtk::HeaderBar,
    main_menu: MainMenu,
    filter_menu: FilterMenu,
    add_media_btn: gtk::Button,
    status_box: gtk::Box,
    status_bar: gtk::Statusbar,
    icon_scroll: gtk::ScrolledWindow,
    icon_view: gtk::IconView,
    paned: gtk::Paned,

    placeholder: gdk_pixbuf::Pixbuf,
    media: RefCell<HashMap<Hash, gtk::TreeIter>>,
    media_store: gtk::ListStore,

    map: osmgpsmap::Map,

    status_media_scan: u32,
}

impl Main {
    const COL_NAME: u32 = 0;
    const COL_PIXBUF: u32 = 1;

    pub fn new(application: gtk::Application, viewer: SyncHandle) -> Self {
        let status_bar = gtk::Statusbar::new();

        let media_cols =
            &[String::static_type(), gdk_pixbuf::Pixbuf::static_type()];

        // TODO: Figure out how to generate this at the correct size instead of
        //       scaling.
        let placeholder = gdk_pixbuf::Pixbuf::from_inline(PLACEHOLDER, false)
            .unwrap()
            .scale_simple(200, 200, gdk_pixbuf::InterpType::Bilinear)
            .unwrap();

        let inner = Inner {
            window: gtk::ApplicationWindow::new(&application),
            header_bar: gtk::HeaderBar::new(),
            main_menu: MainMenu::new(),
            filter_menu: FilterMenu::new(),
            add_media_btn: gtk::Button::new(),
            status_box: gtk::Box::new(gtk::Orientation::Vertical, 0),
            paned: gtk::Paned::new(gtk::Orientation::Vertical),
            icon_view: gtk::IconView::new(),
            icon_scroll: gtk::ScrolledWindow::new::<
                gtk::Adjustment,
                gtk::Adjustment,
            >(None, None),
            map: osmgpsmap::Map::new(),

            placeholder,
            media: Default::default(),
            media_store: gtk::ListStore::new(media_cols),

            status_media_scan: status_bar.get_context_id("media-scan"),

            viewer: RefCell::new(viewer),
            status_bar,
            application,
        };

        Main(Rc::new(inner))
    }

    fn about(&self) {
        // TODO: Fill the rest of this out.

        let authors = env!("CARGO_PKG_AUTHORS")
            .split(":")
            .map(String::from)
            .collect();

        let dialog = gtk::AboutDialogBuilder::new()
            .transient_for(&self.0.window)
            .program_name("Roadtrip")
            .version(env!("CARGO_PKG_VERSION"))
            .title("About")
            .comments("A media player for dashcams and other geotagged content")
            .authors(authors)
            .build();

        dialog.run();
        dialog.close();
    }

    fn import(&self, param: Option<&glib::Variant>) {
        let param = param.expect("import activated without parameter");
        let path_str = param
            .get_str()
            .expect("import activated with non-str parameter");

        self.0.viewer.borrow_mut().scan_media(path_str).unwrap();
    }

    fn choose_import(&self) {
        let dialog = gtk::FileChooserNativeBuilder::new()
            .select_multiple(true)
            .transient_for(&self.0.window)
            .title("Import From")
            .action(gtk::FileChooserAction::SelectFolder)
            .build();

        if dialog.run() != gtk::ResponseType::Accept {
            return;
        }

        let filenames = dialog.get_filenames();
        for filename in filenames {
            let name_str = filename
                .into_os_string()
                .into_string()
                .expect("path was not valid UTF-8");

            self.0
                .application
                .activate_action("import", Some(&name_str.to_variant()));
        }
    }

    pub fn actions(&self) {
        let about = gio::SimpleAction::new("about", None);
        about.connect_activate(
            clone!(@weak self as this => move |_, _| this.about()),
        );
        self.0.application.add_action(&about);

        let choose_import = gio::SimpleAction::new("choose-import", None);
        choose_import.connect_activate(
            clone!(@weak self as this => move |_, _| this.choose_import()),
        );
        self.0.application.add_action(&choose_import);

        let import = gio::SimpleAction::new(
            "import",
            Some(&String::static_variant_type()),
        );
        import.connect_activate(
            clone!(@weak self as this => move |_, v| this.import(v)),
        );
        self.0.application.add_action(&import);
    }

    fn filter(&self) {
        let inner = &self.0;

        let opt_before = inner
            .filter_menu
            .hide_before
            .get_date()
            .map(Self::date_to_midnight_local)
            .as_ref()
            .map(|b| b.to_utc().expect("tz convert"))
            .map(Self::glib_datetime_to_chrono);

        let opt_after = inner
            .filter_menu
            .hide_after
            .get_date()
            .map(Self::date_to_midnight_local)
            .map(|a| a.add_days(1).expect("days out of range"))
            .as_ref()
            .map(|a| a.to_utc().expect("tz convert"))
            .map(Self::glib_datetime_to_chrono);

        let mut filter = Filter::default();
        if let Some(before) = opt_before {
            filter = filter.start(before);
        }

        if let Some(after) = opt_after {
            filter = filter.end(after);
        }

        inner.viewer.borrow_mut().filter(filter).unwrap();
    }

    fn glib_datetime_to_chrono(
        date: glib::DateTime,
    ) -> chrono::DateTime<chrono::Utc> {
        assert_eq!(date.get_timezone(), Some(glib::TimeZone::new_utc()));

        let (year, month, day) = date.get_ymd();
        let cyear: i32 = year.try_into().expect("year out of range");
        let cmonth: u32 = month.try_into().expect("month out of range");
        let cday: u32 = day.try_into().expect("day out of range");

        let chour: u32 = date.get_hour().try_into().expect("hour out of range");
        let cminute: u32 =
            date.get_minute().try_into().expect("minute out of range");
        let csecond: u32 =
            date.get_second().try_into().expect("second out of range");

        let naive = chrono::NaiveDate::from_ymd(cyear, 1 + cmonth, cday)
            .and_hms(chour, cminute, csecond);

        chrono::DateTime::from_utc(naive, chrono::Utc)
    }

    fn date_to_midnight_local(date: glib::Date) -> glib::DateTime {
        use glib::DateMonth::*;

        let month = match date.get_month() {
            January => 0,
            February => 1,
            March => 2,
            April => 3,
            May => 4,
            June => 5,
            July => 6,
            August => 7,
            September => 8,
            October => 9,
            November => 10,
            December => 11,
            _ => panic!("month out of range"),
        };

        glib::DateTime::new_local(
            date.get_year().into(),
            month,
            date.get_day().into(),
            0,
            0,
            0.,
        )
    }

    pub fn build(&self) {
        let inner = &self.0;

        let icon_img = gdk_pixbuf::Pixbuf::from_inline(ICON, false).unwrap();
        inner.window.set_icon(Some(&icon_img));

        inner.main_menu.build();
        inner.filter_menu.build();

        inner.window.set_default_size(800, 600);

        inner.filter_menu.pop.connect_closed(
            clone!(@weak self as this => move |_| this.filter()),
        );

        inner.add_media_btn.set_label("Import");
        inner
            .add_media_btn
            .set_action_name(Some("app.choose-import"));

        inner.header_bar.set_show_close_button(true);
        inner.header_bar.set_title(Some("Roadtrip"));
        inner.header_bar.pack_end(&inner.main_menu.btn);
        inner.header_bar.pack_end(&inner.filter_menu.btn);
        inner.header_bar.pack_start(&inner.add_media_btn);

        inner.window.set_titlebar(Some(&inner.header_bar));

        inner
            .icon_scroll
            .set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);

        inner.icon_view.set_model(Some(&inner.media_store));
        inner.icon_view.set_text_column(Self::COL_NAME as i32);
        inner.icon_view.set_pixbuf_column(Self::COL_PIXBUF as i32);
        inner.icon_view.set_item_width(210);
        inner.icon_scroll.add(&inner.icon_view);

        inner.map.layer_add(&osmgpsmap::MapOsd::new());

        inner.paned.pack1(&inner.map, true, false);
        inner.paned.pack2(&inner.icon_scroll, true, false);

        inner.status_box.add(&inner.paned);
        inner.status_box.set_child_expand(&inner.paned, true);
        inner.status_box.set_child_fill(&inner.paned, true);

        let foo = inner.status_bar.get_context_id("foo");
        inner.status_bar.push(foo, "hello world");
        inner
            .status_box
            .pack_end(&inner.status_bar, false, false, 0);

        inner.window.add(&inner.status_box);
    }

    pub fn show_all(&self) {
        self.0.window.show_all();
    }

    pub fn event(&self, event: Event) {
        match event {
            Event::MediaScanStarted => self.event_media_scan_started(),
            Event::MediaScanCompleted => self.event_media_scan_completed(),
            Event::MediaScanError(err) => self.event_media_scan_error(err),
            Event::FilterChanged => self.event_filter_changed(),
            Event::FilterMatched(media) => self.event_filter_matched(media),
            Event::Thumbnails(thumbs) => self.event_thumbnails(thumbs),
            _ => eprintln!("EVT: {:?}", event),
        }
    }

    fn event_media_scan_started(&self) {
        let inner = &self.0;
        let ctx = inner.status_media_scan;
        inner.status_bar.remove_all(ctx);
        inner.status_bar.push(ctx, "Media scan started...");
    }

    fn event_media_scan_completed(&self) {
        let inner = &self.0;
        let ctx = inner.status_media_scan;
        inner.status_bar.remove_all(ctx);
        inner.status_bar.push(ctx, "Media scan complete");
    }

    fn event_media_scan_error(&self, error: IngestError) {
        let inner = &self.0;
        let ctx = inner.status_media_scan;
        inner.status_bar.remove_all(ctx);
        let msg = format!("Error scanning {}", error.path().to_string_lossy());
        inner.status_bar.push(ctx, &msg);
    }

    fn event_filter_changed(&self) {
        self.0.map.polygon_remove_all();
        self.0.media.borrow_mut().clear();
        self.0.media_store.clear();
    }

    fn event_filter_matched(&self, media: Media) {
        let inner = &self.0;
        let file_name = match media.path().file_name().and_then(|x| x.to_str())
        {
            Some(f) => f,
            None => return, // TODO: Log this?
        };

        let poly = MapPolygon::new();
        let track = poly.get_track().unwrap();
        for point in media.geometry().iter() {
            let mut map_point = MapPoint::new_degrees(
                point.latitude() as f32,
                point.longitude() as f32,
            );

            track.insert_point(&mut map_point, track.n_points());
        }

        inner.map.polygon_add(&poly);

        let iter = inner.media_store.insert_with_values(
            None,
            &[Self::COL_NAME, Self::COL_PIXBUF],
            &[&file_name, &inner.placeholder],
        );
        inner.media.borrow_mut().insert(media.hash().clone(), iter);
    }

    fn event_thumbnails(&self, thumbs: Thumbnails) {
        let media = self.0.media.borrow();

        let iter = match media.get(thumbs.media_hash()) {
            Some(i) => i,
            None => return,
        };

        let file = thumbs.into_files().next().unwrap();

        let pixbuf = gdk_pixbuf::Pixbuf::from_stream(
            &gio::ReadInputStream::new_seekable(file),
            None::<&gio::Cancellable>,
        )
        .unwrap();

        self.0.media_store.set_value(
            &iter,
            Self::COL_PIXBUF,
            &glib::Value::from(&pixbuf),
        );
    }
}
