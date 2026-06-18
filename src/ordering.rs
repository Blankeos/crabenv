use std::cmp::Ordering;

/// Well-known environment variables that should always appear before
/// provider/context-prefixed variables. Keep this short and explicit so the
/// order is deterministic instead of depending on a project-specific prefix.
const STANDARD_ENV_ORDER: &[&str] = &[
    "NODE_ENV",
    "CI",
    "APP_ENV",
    "ENV",
    "ENVIRONMENT",
    "VERCEL_ENV",
    "RAILWAY_ENV",
    "PORT",
    "HOST",
    "HOSTNAME",
    "LOG_LEVEL",
    "DEBUG",
    "TZ",
];

const COMPOUND_PREFIXES: &[&str] = &[
    "NEXT_PUBLIC",
    "NUXT_PUBLIC",
    "PUBLIC",
    "VITE",
    "EXPO_PUBLIC",
    "REACT_APP",
];

pub fn compare_env_names(left: &str, right: &str) -> Ordering {
    match (standard_rank(left), standard_rank(right)) {
        (Some(left_rank), Some(right_rank)) => left_rank.cmp(&right_rank).then(left.cmp(right)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => {
            let left_group = env_group(left);
            let right_group = env_group(right);
            left_group.cmp(&right_group).then_with(|| left.cmp(right))
        }
    }
}

pub fn sort_env_names(names: &mut [String]) {
    names.sort_by(|left, right| compare_env_names(left, right));
}

pub fn env_group(name: &str) -> String {
    if standard_rank(name).is_some() {
        return "__standard__".to_string();
    }

    for prefix in COMPOUND_PREFIXES {
        if name == *prefix || name.starts_with(&format!("{prefix}_")) {
            return (*prefix).to_string();
        }
    }

    name.split('_').next().unwrap_or(name).to_string()
}

fn standard_rank(name: &str) -> Option<usize> {
    STANDARD_ENV_ORDER
        .iter()
        .position(|standard| *standard == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_envs_sort_first_in_explicit_order() {
        let mut names = vec![
            "S3_BUCKET".to_string(),
            "CI".to_string(),
            "RESEND_API_KEY".to_string(),
            "NODE_ENV".to_string(),
        ];

        sort_env_names(&mut names);

        assert_eq!(names, vec!["NODE_ENV", "CI", "RESEND_API_KEY", "S3_BUCKET"]);
    }

    #[test]
    fn provider_prefixes_stay_together() {
        let mut names = vec![
            "S3_SECRET_ACCESS_KEY".to_string(),
            "RESEND_FROM".to_string(),
            "S3_BUCKET_NAME".to_string(),
            "RESEND_API_KEY".to_string(),
        ];

        sort_env_names(&mut names);

        assert_eq!(
            names,
            vec![
                "RESEND_API_KEY",
                "RESEND_FROM",
                "S3_BUCKET_NAME",
                "S3_SECRET_ACCESS_KEY",
            ]
        );
    }
}
