#![no_std]
#![feature(type_alias_impl_trait, const_async_blocks)]
#![warn(
    clippy::complexity,
    clippy::correctness,
    clippy::perf,
    clippy::style,
    clippy::undocumented_unsafe_blocks,
    rust_2018_idioms
)]

use asr::{
    future::{next_tick, retry},
    game_engine::unity::il2cpp::{Module, Version, UnityPointer, Image},
    time::Duration,
    timer::{self, TimerState},
    watcher::Watcher,
    Process,
};

asr::panic_handler!();
asr::async_main!(nightly);

const PROCESS_NAMES: &[&str] = &["PAC-MAN WORLD Re-PAC.exe"];

async fn main() {
    let settings = Settings::register();

    loop {
        // Hook to the target process
        let process = retry(|| PROCESS_NAMES.iter().find_map(|name| Process::attach(name))).await;

        process
            .until_closes(async {
                // Once the target has been found and attached to, set up some default watchers
                let mut watchers = Watchers::default();

                // Perform memory scanning to look for the addresses we need
                let memory = retry(|| Memory::init(&process)).await;

                loop {
                    // Splitting logic. Adapted from OG LiveSplit:
                    // Order of execution
                    // 1. update() will always be run first. There are no conditions on the execution of this action.
                    // 2. If the timer is currently either running or paused, then the isLoading, gameTime, and reset actions will be run.
                    // 3. If reset does not return true, then the split action will be run.
                    // 4. If the timer is currently not running (and not paused), then the start action will be run.
                    update_loop(&process, &memory, &mut watchers);

                    let timer_state = timer::state();
                    if timer_state == TimerState::Running || timer_state == TimerState::Paused {
                        if let Some(is_loading) = is_loading(&watchers, &settings) {
                            if is_loading {
                                timer::pause_game_time()
                            } else {
                                timer::resume_game_time()
                            }
                        }

                        if let Some(game_time) = game_time(&watchers, &settings, &memory) {
                            timer::set_game_time(game_time)
                        }

                        if reset(&watchers, &settings) {
                            timer::reset()
                        } else if split(&watchers, &settings) {
                            timer::split()
                        }
                    }

                    if timer::state() == TimerState::NotRunning && start(&watchers, &settings) {
                        timer::start();
                        timer::pause_game_time();

                        if let Some(is_loading) = is_loading(&watchers, &settings) {
                            if is_loading {
                                timer::pause_game_time()
                            } else {
                                timer::resume_game_time()
                            }
                        }
                    }

                    next_tick().await;
                }
            })
            .await;
    }
}

#[derive(asr::user_settings::Settings)]
struct Settings {
    #[default = true]
    /// => Enable auto start
    start: bool,
    #[default = true]
    /// 1.1 - Buccaneer Beach
    buccaneer_beach: bool,
    #[default = true]
    /// 1.2 - Corsair's Cove
    corsair_cove: bool,
    #[default = true]
    /// 1.3 - Crazy Cannonade
    crazy_cannonade: bool,
    #[default = true]
    /// 1.4 - HMS Windbag
    hms_windbag: bool,
    #[default = true]
    /// 2.1 - Crisis Cavern
    crisis_cavern: bool,
    #[default = true]
    /// 2.2 - Manic Mines
    manic_mines: bool,
    #[default = true]
    /// 2.3 - Anubis Rex
    anubis_rex: bool,
    #[default = true]
    /// 3.1 - Space Race
    space_race: bool,
    #[default = true]
    /// 3.2 - Far Out
    far_out: bool,
    #[default = true]
    /// 3.3 - Gimme Space
    gimme_space: bool,
    #[default = true]
    /// 3.4 - King Galaxian
    king_galaxian: bool,
    #[default = true]
    /// 4.1 - Clowning Around
    clowning_around: bool,
    #[default = true]
    /// 4.2 - Barrel Blast
    barrel_blast: bool,
    #[default = true]
    /// 4.3 - Barrel Dizzy
    barrel_dizzy: bool,
    #[default = true]
    /// 4.4 - Clown Prix
    clown_prix: bool,
    #[default = true]
    /// 5.1 - Perilous Pipes
    perilous_pipes: bool,
    #[default = true]
    /// 5.2 - Under Pressure
    under_pressure: bool,
    #[default = true]
    /// 5.3 - Down the Tubes
    down_the_tubes: bool,
    #[default = true]
    /// 5.4 - Krome Keeper
    krome_keeper: bool,
    #[default = true]
    /// 6.1 - Ghostly Garden
    ghostly_garden: bool,
    #[default = true]
    /// 6.2 - Creepy Catacombs
    creepy_catacombs: bool,
    #[default = true]
    /// 6.3 - Grave Danger
    grave_danger: bool,
    #[default = true]
    /// 6.4 - Toc-Man's Lair
    toc_man_lair: bool,
}

#[derive(Default)]
struct Watchers {
    is_loading: Watcher<bool>,
    level_id: Watcher<u32>,
    level_id_unfiltered: Watcher<u32>,
    tocman_qte: Watcher<bool>,
}

struct Memory {
    il2cpp_module: Module,
    game_assembly: Image,
    is_loading: UnityPointer<2>,
    level_id: UnityPointer<2>,
    is_loading_2: UnityPointer<2>,
    tocman_qte: UnityPointer<2>,
}

impl Memory {
    fn init(game: &Process) -> Option<Self> {
        let il2cpp_module = Module::attach(game, Version::V2020)?;
        let game_assembly = il2cpp_module.get_default_image(game)?;

        let is_loading = UnityPointer::new("SceneManager", 1, &["s_sInstance", "m_bProcessing"]);
        let level_id = UnityPointer::new("SceneManager", 1, &["s_sInstance", "m_eCurrentScene"]);

        let is_loading_2 = UnityPointer::new("GameStateManager", 1, &["s_sInstance", "loadScr"]);
        let tocman_qte = UnityPointer::new("BossTocman", 1, &["s_sInstance", "m_qteSuccess"]);

        Some(Self {
            il2cpp_module,
            game_assembly,
            is_loading,
            level_id,
            is_loading_2,
            tocman_qte,
        })
    }
}

fn update_loop(game: &Process, addresses: &Memory, watchers: &mut Watchers) {
    watchers.is_loading.update_infallible(
        addresses.is_loading.deref::<bool>(game, &addresses.il2cpp_module, &addresses.game_assembly).unwrap_or_default()
            || addresses
                .is_loading_2
                .deref::<u64>(game, &addresses.il2cpp_module, &addresses.game_assembly)
                .unwrap_or_default()
                != 0,
    );

    let cur_level = addresses.level_id.deref::<u32>(game, &addresses.il2cpp_module, &addresses.game_assembly).unwrap_or_default();

    watchers.level_id.update_infallible({
        if cur_level > 100 && cur_level <= 604 {
            cur_level
        } else {
            match watchers.level_id.pair {
                Some(x) => x.current,
                _ => 101,
            }
        }
    });

    watchers.level_id_unfiltered.update_infallible(cur_level);

    watchers
        .tocman_qte
        .update_infallible(addresses.tocman_qte.deref(game, &addresses.il2cpp_module, &addresses.game_assembly).unwrap_or_default());
}

fn start(watchers: &Watchers, settings: &Settings) -> bool {
    if !settings.start {
        return false;
    }

    watchers
        .level_id_unfiltered
        .pair
        .is_some_and(|val| val.current == 4)
        && watchers
            .is_loading
            .pair
            .is_some_and(|val| val.changed_to(&true))
}

fn split(watchers: &Watchers, settings: &Settings) -> bool {
    let Some(level_id_unfiltered) = &watchers.level_id_unfiltered.pair else { return false };
    let Some(level_id) = &watchers.level_id.pair else { return false };

    if level_id_unfiltered.changed_to(&1)
        && (level_id_unfiltered.old == 3 || level_id_unfiltered.old > 1000)
    {
        match level_id.current {
            101 => settings.buccaneer_beach,
            102 => settings.corsair_cove,
            103 => settings.crazy_cannonade,
            104 => settings.hms_windbag,
            201 => settings.crisis_cavern,
            202 => settings.manic_mines,
            203 => settings.anubis_rex,
            301 => settings.space_race,
            302 => settings.far_out,
            303 => settings.gimme_space,
            304 => settings.king_galaxian,
            401 => settings.clowning_around,
            402 => settings.barrel_blast,
            403 => settings.barrel_dizzy,
            404 => settings.clown_prix,
            501 => settings.perilous_pipes,
            502 => settings.under_pressure,
            503 => settings.down_the_tubes,
            504 => settings.krome_keeper,
            601 => settings.ghostly_garden,
            602 => settings.creepy_catacombs,
            603 => settings.grave_danger,
            604 => settings.toc_man_lair,
            _ => false,
        }
    } else {
        level_id_unfiltered.current == 604
            && !level_id_unfiltered.changed()
            && settings.toc_man_lair
            && watchers
                .tocman_qte
                .pair
                .is_some_and(|val| val.changed_to(&true))
    }
}

fn reset(_watchers: &Watchers, _settings: &Settings) -> bool {
    false
}

fn is_loading(watchers: &Watchers, _settings: &Settings) -> Option<bool> {
    Some(watchers.is_loading.pair?.current)
}

fn game_time(_watchers: &Watchers, _settings: &Settings, _addresses: &Memory) -> Option<Duration> {
    None
}
