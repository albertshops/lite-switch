use crate::{history::History, search};
use gtk::gio::{self, prelude::*};

#[derive(Clone)]
pub struct Application {
    pub id: String,
    pub name: String,
    pub wm_class: Option<String>,
    pub executable: Option<String>,
    pub info: gio::AppInfo,
}

pub fn discover() -> Vec<Application> {
    let mut applications: Vec<_> = gio::AppInfo::all()
        .into_iter()
        .filter(|info| info.should_show())
        .filter_map(|info| {
            Some(Application {
                id: info.id()?.to_string(),
                name: info.display_name().to_string(),
                wm_class: info
                    .downcast_ref::<gio::DesktopAppInfo>()
                    .and_then(|desktop| desktop.startup_wm_class())
                    .map(|value| value.to_string()),
                executable: info
                    .executable()
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned()),
                info,
            })
        })
        .collect();
    applications.sort_by(|a, b| a.id.cmp(&b.id));
    applications.dedup_by(|a, b| a.id == b.id);
    applications
}

pub fn ranked<'a>(
    apps: &'a [Application],
    query: &str,
    history: &History,
    now: u64,
) -> Vec<&'a Application> {
    let empty = search::normalized(query).is_empty();
    let score = |id: &str| {
        history
            .apps
            .get(id)
            .map_or(0.0, |r| search::usage_score(r.count, r.latest, now))
    };
    let mut matches: Vec<_> = apps
        .iter()
        .filter_map(|app| {
            search::match_name(query, &app.name).map(|matched| (app, matched, score(&app.id)))
        })
        .collect();
    matches.sort_by(|(a, am, au), (b, bm, bu)| {
        let usage = bu.total_cmp(au);
        let name = search::normalized(&a.name)
            .cmp(&search::normalized(&b.name))
            .then_with(|| a.id.cmp(&b.id));
        if empty {
            usage.then(name)
        } else {
            am.tier
                .cmp(&bm.tier)
                .then(am.gap.cmp(&bm.gap))
                .then(usage)
                .then(am.length.cmp(&bm.length))
                .then(name)
        }
    });
    matches.into_iter().map(|(app, _, _)| app).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::Record;

    fn app(id: &str, name: &str) -> Application {
        Application {
            id: id.into(),
            name: name.into(),
            wm_class: None,
            executable: None,
            info: gio::AppInfo::create_from_commandline(
                "/bin/true",
                Some(name),
                gio::AppInfoCreateFlags::NONE,
            )
            .unwrap(),
        }
    }

    #[test]
    fn usage_cannot_promote_a_weaker_tier() {
        let apps = vec![
            app("prefix", "Code Editor"),
            app("exact", "Code"),
            app("fuzzy", "Commander"),
            app("word", "Visual Code"),
        ];
        let mut history = History::default();
        history.apps.insert(
            "prefix".into(),
            Record {
                count: u64::MAX,
                latest: 100,
            },
        );
        assert_eq!(
            ranked(&apps, "code", &history, 100)
                .iter()
                .map(|a| a.id.as_str())
                .collect::<Vec<_>>(),
            vec!["exact", "prefix", "word", "fuzzy"]
        );
    }

    #[test]
    fn empty_query_uses_history_then_deterministic_names() {
        let apps = vec![app("a", "Alpha"), app("b", "Beta"), app("c", "Gamma")];
        let mut history = History::default();
        history.apps.insert(
            "c".into(),
            Record {
                count: 2,
                latest: 100,
            },
        );
        assert_eq!(
            ranked(&apps, "", &history, 100)
                .iter()
                .map(|a| a.id.as_str())
                .collect::<Vec<_>>(),
            vec!["c", "a", "b"]
        );
    }
}
