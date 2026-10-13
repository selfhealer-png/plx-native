//! Activation ports: a deck promises playback only under `DeckPress::Play`; discovery, even an episode, never does.
use super::*;
use plx_media::route::DeckPress;
use plx_ui::fixture::{FixtureArg, FixtureMeasure};
use plx_ui::focus::FocusEngine;
use plx_machine::machine::{Host, InputOwner, PressId, PressRead, Tick};

struct TestHost;

#[derive(Clone, Copy)]
struct Views<'a> {
    listing: plx_data::stores::browse::ListingView<'a>,
    directory: plx_data::stores::browse::DirectoryView<'a>,
    hubs: plx_data::stores::browse::HubsView<'a>,
}
impl Host for TestHost {
    type Arg = FixtureArg;
    type Fx = AppFx;
    type Msg = AppMsg;
    type Elem = u32;
    type Views<'a> = Views<'a>;
    type Init = FixtureArg;
    type Memory = PageMemory;
}
impl LibraryLike for TestHost {
    fn listing<'a>(cx: &Cx<'a, Self>) -> plx_data::stores::browse::ListingView<'a> {
        cx.views.listing
    }
    fn directory<'a>(cx: &Cx<'a, Self>) -> plx_data::stores::browse::DirectoryView<'a> {
        cx.views.directory
    }
    fn section_hubs<'a>(cx: &Cx<'a, Self>) -> plx_data::stores::browse::HubsView<'a> {
        cx.views.hubs
    }
}

#[test]
fn shelf_activate_and_hold_keep_the_deck_promise_and_engine_item_identity() {
    let _guard = plx_base::testlock::serial();
    let session = plx_plex::plex::session::TempSession::new("library-shelf-actions");
    session.watching("u-library-shelf-actions");
    let stores = plx_data::stores::Stores::default();
    stores.browse.borrow_mut().seed_two_source_table_for_test();
    let mut directory = plx_data::stores::browse::DirectorySnapshot::default();
    stores.capture_browse(&mut directory);
    stores.browse_run(BrowseCmd::SetCur(0));
    {
        let mut browse = stores.browse.borrow_mut();
        browse.seed_items_for_test(12);
        browse.seed_shelves_for_test(
            0,
            &["movie.inprogress.1", "tv.recentlyreleased.1"],
            3,
        );
        browse.seed_landscape_for_test(0, "Synthetic show");
    }
    let publication = stores.capture_browse(&mut directory);
    let listing = publication.listing;
    let hubs = publication.section_hubs;
    let listing_id = listing.view().id().unwrap();
    let hubs_id = hubs.view().id().unwrap();
    assert_eq!(
        (listing_id.epoch, listing_id.sid, listing_id.section),
        (hubs_id.epoch, hubs_id.sid, hubs_id.section)
    );
    assert_eq!(hubs.view().shelves().len(), 2);
    let entry = EntryId(82);
    let owner = InputOwner::Entry(entry);
    let mut engine = FocusEngine::new();
    let cx = |engine: &FocusEngine<u32>| Cx::<TestHost> {
        views: Views {
            listing: listing.view(),
            directory: directory.view(),
            hubs: hubs.view(),
        },
        tick: Tick::default(),
        measure: &FixtureMeasure,
        focus: engine.read(owner),
        press: PressRead {
            scale: 0.85,
            is_long: true,
            held_ms: None,
            owner: None,
        },
        owner,
    };
    let mut page = LibraryScreen::new(entry, InstanceId(20), SecKind::Movie);
    page.sync(&cx(&engine));
    assert_eq!(page.shelves.len(), 2);
    for row in 0..2 {
        let item = &hubs.view().shelves()[row].items[1];
        assert_eq!(
            item.kind, 3,
            "discovery episode is the important non-play control"
        );
        assert!(hubs.view().shelves()[row].landscape);
        let from_deck = row == 0;
        assert_eq!(hubs.view().shelves()[row].is_continue, from_deck);
        let key = page.key(page.shelves[row].elems[1]);
        engine.set(owner, key, Some(page.shelves[row].group), By::Restore);
        // OK on a deck card plays only under the Play setting (the default opens the page); the
        // hold's menu is the same request either way.
        for (mode, held) in [
            (DeckPress::Play, false),
            (DeckPress::Details, false),
            (DeckPress::Play, true),
            (DeckPress::Details, true),
        ] {
            let prior = plx_media::route::deck_press();
            plx_media::route::restore_deck_press(mode);
            struct Back(DeckPress);
            impl Drop for Back {
                fn drop(&mut self) {
                    plx_media::route::restore_deck_press(self.0);
                }
            }
            let _back = Back(prior);
            let mut out = Vec::new();
            let mut present = plx_machine::present::Present::new();
            let event = if held {
                ScreenEvent::PressHold(PressId(7))
            } else {
                ScreenEvent::Activate(key.elem)
            };
            let handled = page.step(
                &event,
                &cx(&engine),
                &mut Effects::new(&mut out, MachineId::Instance(InstanceId(20)), &mut present),
            );
            assert_eq!(handled, Handled::Yes);
            let requests: Vec<_> = out
                .into_iter()
                .filter_map(|effect| match effect.fx {
                    Fx::App(AppFx::Library(req)) => Some(req),
                    _ => None,
                })
                .collect();
            let expected = if held {
                LibraryReq::ItemMenu {
                    sid: item.sid,
                    rk: item.rk.clone(),
                    from_deck,
                }
            } else if from_deck && mode.press_plays() {
                LibraryReq::Play {
                    sid: item.sid,
                    rk: item.rk.clone(),
                    resume_ns: 0,
                }
            } else {
                // the page request says WHICH shelf the press came from: a deck episode opens its
                // show, a discovery shelf's opens the episode (`activate_deck_card`)
                LibraryReq::Detail {
                    sid: item.sid,
                    rk: item.rk.clone(),
                    from_deck,
                }
            };
            assert_eq!(requests, vec![expected]);
            assert_eq!(engine.current(owner), Some(key));
        }
    }
}

/// A promoted collection shelf's heading is plain text and the shelf ends in a MORE card: DOWN
/// from the shelf above lands on the collection's cards, RIGHT along it reaches MORE, OK on MORE
/// opens the collection, and MORE is a pointer stop like any card. Every other shelf is unchanged.
#[test]
fn a_collection_shelf_ends_in_a_more_card_that_opens_the_collection() {
    use plx_ui::focus::Outcome;
    use plx_ui::screen::DrawFrame;
    let _guard = plx_base::testlock::serial();
    let session = plx_plex::plex::session::TempSession::new("library-linked-heading");
    session.watching("u-library-linked-heading");
    let stores = plx_data::stores::Stores::default();
    stores.browse.borrow_mut().seed_two_source_table_for_test();
    let mut directory = plx_data::stores::browse::DirectorySnapshot::default();
    stores.capture_browse(&mut directory);
    stores.browse_run(BrowseCmd::SetCur(0));
    {
        let mut browse = stores.browse.borrow_mut();
        browse.seed_items_for_test(12);
        browse.seed_named_shelves_for_test(0, &[
            ("movie.recentlyadded.1", "/hubs/sections/1/recentlyAdded", "Recently Added"),
            ("custom.collection.1.50001.50001", "/library/collections/50001/children", "Toy Story Collection"),
        ], 5);
    }
    let publication = stores.capture_browse(&mut directory);
    let listing = publication.listing;
    let hubs = publication.section_hubs;
    let entry = EntryId(83);
    let owner = InputOwner::Entry(entry);
    let mut engine = FocusEngine::new();
    let cx = |engine: &FocusEngine<u32>| Cx::<TestHost> {
        views: Views { listing: listing.view(), directory: directory.view(), hubs: hubs.view() },
        tick: Tick::default(),
        measure: &FixtureMeasure,
        focus: engine.read(owner),
        press: PressRead::default(),
        owner,
    };
    let mut page = LibraryScreen::new(entry, InstanceId(21), SecKind::Movie);
    page.sync(&cx(&engine));
    assert_eq!(page.shelves.len(), 2);
    assert!(page.shelves[0].heading.is_none(), "an ordinary shelf has no MORE card");
    let more = page.shelves[1].heading_elem().expect("the collection shelf is linked");
    assert_eq!(page.shelves[1].elems.len(), 6, "five members, then MORE");
    assert_eq!(page.shelves[1].elems.last(), Some(&more));
    assert_eq!(page.shelves[0].elems.len(), 5);
    assert_eq!(page.shelves[1].id, "custom.collection.1.50001.50001",
        "the shelf keeps its hub identity for page memory");
    assert_eq!(Focusable::<TestHost>::group_of(&page, &more, &cx(&engine)), Some(page.shelves[1].group),
        "MORE belongs to its shelf's own group");

    let mut links = Vec::new();
    Screen::<TestHost>::links(&page, &mut links);
    let step = |engine: &mut FocusEngine<u32>, dir| engine.move_dir(owner, &page, &links, dir, &cx(engine));

    // DOWN from the shelf above goes straight to the collection's cards.
    engine.set(owner, page.key(page.shelves[0].elems[1]), Some(page.shelves[0].group), By::Restore);
    let Outcome::Moved { to, .. } = step(&mut engine, Dir::Down) else { panic!("DOWN must move") };
    assert!(page.shelves[1].elems.contains(&to.elem) && to.elem != more,
        "DOWN lands on a member card, not a heading");

    // RIGHT from the last member reaches MORE.
    engine.set(owner, page.key(page.shelves[1].elems[4]), Some(page.shelves[1].group), By::Restore);
    let Outcome::Moved { to, .. } = step(&mut engine, Dir::Right) else { panic!("RIGHT must reach MORE") };
    assert_eq!(to, page.key(more));

    // OK on MORE opens the collection page.
    let mut out = Vec::new();
    let mut present = plx_machine::present::Present::new();
    let handled = page.step(&ScreenEvent::Activate(more), &cx(&engine),
        &mut Effects::new(&mut out, MachineId::Instance(InstanceId(21)), &mut present));
    assert_eq!(handled, Handled::Yes);
    let pushed: Vec<_> = out.into_iter().filter_map(|effect| match effect.fx {
        Fx::App(AppFx::Content(crate::registry::ContentReq::Push(arg))) => Some(arg),
        Fx::App(AppFx::Library(req)) => panic!("MORE must not emit {req:?}"),
        _ => None,
    }).collect();
    assert_eq!(pushed.len(), 1);
    assert!(matches!(&pushed[0], crate::registry::ContentArg::Collection(id)
        if id.rk == "50001" && id.sec == 1 && id.tag == 0 && id.name == "Toy Story Collection"));

    // Pointer: MORE registers a stop as a card does.
    page.relayout(engine.current(owner));
    let context = cx(&engine);
    let mut frame = DrawFrame::new(&context, plx_ui::Painter::root());
    page.record_stops(&mut frame);
    let stops = frame.into_stops();
    assert!(stops.iter().any(|stop| stop.key.elem == more), "MORE has a stop");
}
