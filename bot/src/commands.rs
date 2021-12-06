use std::borrow::Cow;
use std::cmp::{max, min};
use std::fmt::Write;
use std::num::ParseIntError;
use std::str::FromStr;

use clap::{App, AppSettings, Arg};
use log::debug;
use url::Url;
use uuid::Uuid;

use msgtools::Ac;

use crate::db::entity::{playlist, Playlist};
use crate::entity::import::ImportError;
use crate::fmt::HtmlDisplayExt;
use crate::player::treepath::TreePathBuf;
use crate::{Bot, Result};

const COMMAND_PREFIX: char = ';';

pub async fn handle_message_event(bot: &mut Bot, ev: &mumble::event::Message) -> Result {
    let name: Cow<_> = match ev.actor {
        None => "<unknown>".into(),
        Some(r) => match bot.client.get_user(r).await? {
            None => "<unknown>".into(),
            Some(user) => user.name().to_string().into(),
        },
    };

    println!("{}: {}", name, ev.message);

    if let Some(msg) = ev.message.strip_prefix(COMMAND_PREFIX) {
        let msg = msg.trim();
        handle_command(bot, ev, msg).await?;
    }

    Ok(())
}

macro_rules! match_commands {
    ($cmde:expr, $bot:expr, $ev:expr, $args:expr, $out:expr, $($cmd:ident)*) => {
        match $cmde {
            $(stringify!($cmd) => $cmd($bot, $ev, $args, &mut $out).await?,)*
            _ => {}
        }
    };
}

async fn handle_command(bot: &mut Bot, ev: &mumble::event::Message, msg: &str) -> Result {
    let cmds = tokenize(msg);

    for cmdline in cmds {
        let cmd = &*cmdline[0];
        let args = &cmdline[1..];
        let mut out = String::new();

        match_commands! {
            cmd, bot, ev, args, out,
            skip pause play list random new newsub web quit
            playlist
        }

        if !out.is_empty() {
            let _ = bot.client.respond(ev, out).await;
        }
    }

    Ok(())
}

fn app_for_command(name: &'static str) -> App {
    App::new(name)
        .setting(AppSettings::DisableVersionFlag)
        .setting(AppSettings::NoBinaryName)
}

macro_rules! unwrap_matches {
    ($matches:ident, $out:expr) => {
        #[allow(unused)]
        let $matches = match $matches {
            Ok(v) => v,
            Err(e) => {
                let text = format!("{}", e).replace('&', "&amp;").replace('<', "&lt;");
                writeln!($out, "<pre>{}</pre>", text).unwrap();
                return Ok(());
            }
        };
    };
}

async fn skip(bot: &Bot, ev: &mumble::event::Message, args: &[String], out: &mut String) -> Result {
    let matches = app_for_command("skip")
        .about("Skip the currently playing track")
        .try_get_matches_from(args.iter());
    unwrap_matches!(matches, out);

    bot.room.proxy().next().await?;

    Ok(())
}

async fn pause(
    bot: &Bot,
    ev: &mumble::event::Message,
    args: &[String],
    out: &mut String,
) -> Result {
    let matches = app_for_command("pause")
        .about("Pause the currently playing track")
        .try_get_matches_from(args.iter());
    unwrap_matches!(matches, out);

    bot.room.proxy().pause().await?;

    Ok(())
}

async fn play(bot: &Bot, ev: &mumble::event::Message, args: &[String], out: &mut String) -> Result {
    let matches = app_for_command("play")
        .about("Start playing the current track")
        .try_get_matches_from(args.iter());
    unwrap_matches!(matches, out);

    bot.room.proxy().play().await?;

    Ok(())
}

async fn list(bot: &Bot, ev: &mumble::event::Message, args: &[String], out: &mut String) -> Result {
    let matches = app_for_command("list")
        .about("List entries of the current playlist")
        .args(&[
            Arg::new("start")
                .value_name("START")
                .about("First row to output")
                .default_value("0"),
            Arg::new("end")
                .value_name("END")
                .about("Last row to output")
                .default_value("+20"),
            Arg::new("expand")
                .short('e')
                .long("expand")
                .value_name("DEPTH")
                .about("Expand nested playlists until depth")
                .default_value("1")
                .default_missing_value("99"),
        ])
        .try_get_matches_from(args.iter());
    unwrap_matches!(matches, out);

    enum End {
        Absolute(usize),
        Relative(usize),
    }

    impl FromStr for End {
        type Err = ParseIntError;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            if s.starts_with("+") {
                Ok(End::Relative(s[1..].parse()?))
            } else {
                Ok(End::Absolute(s.parse()?))
            }
        }
    }

    let start: usize = matches.value_of("start").unwrap().parse().unwrap();
    let end: End = matches.value_of("end").unwrap().parse().unwrap();
    let end = match end {
        End::Absolute(v) => v,
        End::Relative(v) => start + v,
    };

    let pl = match bot.room.proxy().playlist().await {
        Ok(v) => v,
        Err(e) => {
            writeln!(out, "failed to get playlist: {}", e).unwrap();
            return Ok(());
        }
    };

    let max_length = bot.client.max_message_length().await;

    writeln!(out, "{}", pl.html()).unwrap();

    write!(out, "<table><tr><th><u>P</u>os</th><th><u>T</u>itle</th><th><u>A</u>rtist</th><th>A<u>l</u>bum</th></tr>").unwrap();
    write!(out, "<tr><th></th><th></th><th>Shuffle</th></tr>").unwrap();

    if pl.entries().len() > 0 {
        let start = min(start, pl.entries().len() - 1);
        let end = min(max(start, end), pl.entries().len() - 1);

        if start > 0 {
            write!(
                out,
                "<tr><td colspan=\"4\"><i>({} rows omitted)</i></td></tr>",
                start
            )
            .unwrap();
        }

        for (idx, entry) in pl.entries()[start..=end].iter().enumerate() {
            let idx = idx + start;

            match entry.content() {
                playlist::Content::Track(tr) => {
                    let (artist, album) = ("", ""); // TODO
                    write!(
                        out,
                        "<tr><td align=\"right\">{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                        idx,
                        tr.object().title().unwrap_or(""),
                        artist,
                        album
                    )
                    .unwrap();
                }
                playlist::Content::Playlist(pl) => {
                    write!(
                        out,
                        "<tr><td align=\"right\">{}</td><td>{}</td><td>{}</td></tr>",
                        idx,
                        pl.object().title(),
                        //if pl.shuffle() { "yes" } else { "no" },
                        "no",
                    )
                    .unwrap();
                }
            }
        }

        if end < pl.entries().len() - 1 {
            write!(
                out,
                "<tr><td colspan=\"4\"><i>({} rows omitted)</i></td></tr>",
                pl.entries().len() - end - 1
            )
            .unwrap();
        }
    }

    writeln!(out, "</table>").unwrap();
    Ok(())
}

async fn random(
    bot: &Bot,
    ev: &mumble::event::Message,
    args: &[String],
    out: &mut String,
) -> Result {
    let matches = app_for_command("random")
        .about("Toggles random mode on or off")
        .try_get_matches_from(args.iter());
    unwrap_matches!(matches, out);

    let new_random = bot.room.proxy().toggle_random().await?;

    if new_random {
        writeln!(out, "Random mode is now on").unwrap();
    } else {
        writeln!(out, "Random mode is now off").unwrap();
    }

    Ok(())
}

async fn new(bot: &Bot, ev: &mumble::event::Message, args: &[String], out: &mut String) -> Result {
    let matches = app_for_command("new")
        .about("Create a new playlist")
        .args(&[
            Arg::new("name")
                .value_name("NAME")
                .about("Specify the name of the new playlist"),
            Arg::new("force")
                .short('f')
                .long("force")
                .about("Force replace playlist with unsaved changes"),
        ])
        .try_get_matches_from(args.iter());
    unwrap_matches!(matches, out);

    let mut playlist = Ac::new(Playlist::new());

    if let Some(name) = matches.value_of("name") {
        playlist.set_title(name);
    }

    bot.room.proxy().set_playlist(playlist).await?;

    Ok(())
}

async fn newsub(
    bot: &Bot,
    ev: &mumble::event::Message,
    args: &[String],
    out: &mut String,
) -> Result {
    let matches = app_for_command("newsub")
        .about("Attach a new sub-playlist")
        .args(&[
            Arg::new("path")
                .short('p')
                .value_name("PATH")
                .default_value("-")
                .about("The path to the playlist the new one should be attached to"),
            Arg::new("name")
                .value_name("NAME")
                .about("Specify the name of the new playlist"),
        ])
        .try_get_matches_from(args.iter());
    unwrap_matches!(matches, out);

    let path = matches.value_of("path").unwrap();
    let path = match TreePathBuf::from_str(path) {
        Ok(v) => v,
        Err(e) => {
            writeln!(out, "error: {}: {}", e, path).unwrap();
            return Ok(());
        }
    };

    bot.room
        .proxy()
        .add_playlist(Ac::new(Playlist::new()), path)
        .await?;

    Ok(())
}

async fn import(
    bot: &mut Bot,
    ev: &mumble::event::Message,
    args: &[String],
    out: &mut String,
) -> Result {
    let matches = app_for_command("import")
        .about("Import a playlist")
        .args([Arg::new("url")
            .value_name("URL")
            .about("The URL to fetch the playlist from")])
        .try_get_matches_from(args.iter());
    unwrap_matches!(matches, out);

    Ok(())
}

async fn playlist(
    bot: &mut Bot,
    ev: &mumble::event::Message,
    args: &[String],
    out: &mut String,
) -> Result {
    let matches = app_for_command("playlist")
        .about("The playlist management interface")
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .subcommands([
            app_for_command("create")
                .short_flag('C')
                .about("Create a new playlist")
                .args([
                    Arg::new("name")
                        .short('n')
                        .long("name")
                        .about("The name of the playlist to create")
                        .value_name("NAME"),
                    Arg::new("code")
                        .short('c')
                        .long("code")
                        .value_name("CODE")
                        .about("Use the provided code for the playlist"),
                    Arg::new("from")
                        .long("from")
                        .value_name("URL")
                        .about("The source URL to fetch the playlist from"),
                    Arg::new("force")
                        .short('f')
                        .long("force")
                        .about("Imports the playlist even if another one with the same source already exists"),
                    Arg::new("play")
                        .short('p')
                        .long("play")
                        .about("After importing, sets this as the active playlist")
                ]),
            app_for_command("modify")
                .short_flag('M')
                .args([
                    Arg::new("code")
                        .value_name("CODE")
                        .about("The code of the playlist to delete")
                        .required(true),
                    Arg::new("name")
                        .short('n')
                        .long("name")
                        .value_name("NAME")
                        .about("Sets the playlist name to NAME."),
                    Arg::new("track")
                        .short('t')
                        .long("track")
                        .value_name("TRACK")
                        .about("Adds the track with the specified code TRACK")
                        .multiple_occurrences(true),
                    Arg::new("sync")
                        .short('s')
                        .long("sync")
                        .about("Syncs the playlist against the configured external source")
                ]),
            app_for_command("delete")
                .short_flag('R')
                .args([
                    Arg::new("code")
                        .value_name("CODE")
                        .about("The code of the playlist to delete"),
                ]),
            app_for_command("query").short_flag('Q'),
        ])
        .try_get_matches_from(args.iter());
    unwrap_matches!(matches, out);

    match matches.subcommand() {
        Some(("create", matches)) => {
            let name = matches.value_of("name");
            let _code = matches.value_of("code");
            let from = matches.value_of("from");
            let force = matches.is_present("force");
            let play = matches.is_present("play");

            let mut pl = Playlist::new();

            let mut db = match bot.db.acquire().await {
                Ok(v) => v,
                Err(e) => {
                    writeln!(out, "failed to acquire database connection: {}", e).unwrap();
                    return Ok(());
                }
            };

            if let Some(from) = from {
                let url = match Url::parse(from) {
                    Ok(v) => v,
                    Err(e) => {
                        writeln!(out, "failed to parse URL: {}", e).unwrap();
                        return Ok(());
                    }
                };

                if (url.domain() == Some("www.youtube.com") || url.domain() == Some("youtube.com"))
                    && url.path() == "/playlist"
                {
                    let mut list = None;

                    for (k, v) in url.query_pairs() {
                        if k == "list" {
                            list = Some(v);
                        }
                    }

                    if let Some(list) = list {
                        let res: Result<_, ImportError> =
                            Playlist::import_by_youtube_id(&list, &mut *db).await;

                        match res {
                            Ok(v) => {
                                pl = v;
                            }
                            Err(e) => {
                                writeln!(out, "failed to import playlist: {}", e).unwrap();
                                return Ok(());
                            }
                        }
                    } else {
                        writeln!(out, "could not parse YouTube playlist URL").unwrap();
                        return Ok(());
                    }
                } else {
                    writeln!(out, "don't know how to parse this URL").unwrap();
                    return Ok(());
                }
            }

            if pl.object().id().is_some() {
                // existing playlist was loaded from database
                writeln!(out, "found existing playlist in database: {}", pl.html(),).unwrap();
            } else {
                if let Some(name) = name {
                    pl.set_title(name);
                }

                if let Err(e) = pl.save(&mut *db).await {
                    writeln!(out, "failed to save playlist: {}", e).unwrap();
                    return Ok(());
                }

                writeln!(out, "imported {}", pl.html()).unwrap();
            }

            if play {
                let _ = bot.room.proxy().set_playlist(Ac::new(pl)).await;
            }
        }
        Some(("modify", matches)) => {}
        Some(("delete", matches)) => {}
        Some(("query", matches)) => {}
        _ => unreachable!(),
    }

    Ok(())
}

async fn web(
    bot: &mut Bot,
    ev: &mumble::event::Message,
    args: &[String],
    out: &mut String,
) -> Result {
    let matches = app_for_command("web")
        .about("Open the web control interface")
        .try_get_matches_from(args.iter());
    unwrap_matches!(matches, out);

    if let Some(actor) = ev.actor {
        let user = actor.get(&*bot.client.state().await?);

        let user = match user {
            None => {
                // wtf
                writeln!(out, "couldn't find your user data, please reconnect").unwrap();
                return Ok(());
            }
            Some(v) => v,
        };

        let token = Uuid::new_v4();

        debug!("login token {} for user {}", token, user.name());

        // TODO!
        let webroot_url = "https://r2dj.2x.ax";

        bot.client
            .message_user(
                actor,
                &format!(
                    "<a href=\"{}/login?token={}\">Login</a> (this does not work yet)",
                    webroot_url, token
                ),
            )
            .await?;
    }

    Ok(())
}

async fn quit(
    bot: &mut Bot,
    ev: &mumble::event::Message,
    args: &[String],
    out: &mut String,
) -> Result {
    let matches = app_for_command("quit")
        .about("Shut down the bot")
        .try_get_matches_from(args.iter());
    unwrap_matches!(matches, out);

    if let Some(tx) = bot.shutdown_fuse.take() {
        let _ = tx.send(());
    }

    Ok(())
}

// TODO: make this in cmdparser public so I don't have to copy it
/// Tokenize script source, removing comments (starting with `//`).
/// Returns a list of command executions (command + arguments)
fn tokenize(s: &str) -> Vec<Vec<String>> {
    let mut esc = false;
    let mut quoted = false;
    let mut commands = vec![];
    let mut current = vec![];
    let mut sb = String::new();

    fn next_token(sb: &mut String, current: &mut Vec<String>) {
        if !sb.trim().is_empty() {
            current.push((*sb).clone());
        }
        sb.clear();
    }

    fn next_command(sb: &mut String, current: &mut Vec<String>, commands: &mut Vec<Vec<String>>) {
        next_token(sb, current);
        if !current.is_empty() {
            commands.push((*current).clone());
        }
        current.clear();
    }

    for line in s.lines() {
        let get = |i| line.chars().nth(i);

        for (pos, c) in line.chars().enumerate() {
            if esc {
                sb.push(c);
                esc = false;
            // } else if !quoted && c == '/' && get(pos + 1) == Some('/') {
            //     break;
            } else if !quoted && c == ';' {
                next_command(&mut sb, &mut current, &mut commands);
            } else if !quoted && c == ' ' {
                next_token(&mut sb, &mut current);
            } else if c == '"' {
                quoted = !quoted;
            } else if c == '\\' {
                esc = true;
            } else {
                sb.push(c);
            }
        }

        next_command(&mut sb, &mut current, &mut commands);
    }

    commands
}
