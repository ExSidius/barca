use barca_core::models::{AssetDetail, AssetSummary, JobDetail};
use comfy_table::{presets::UTF8_FULL_CONDENSED, Table};

pub fn assets_table(assets: &[AssetSummary]) -> String {
    if assets.is_empty() {
        return "No assets indexed.".to_string();
    }
    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_header(["ID", "Name", "Module", "Function", "Status"]);
    for a in assets {
        let status = a.materialization_status.as_deref().unwrap_or("never run");
        table.add_row([&a.asset_id.to_string(), &a.logical_name, &a.module_path, &a.function_name, status]);
    }
    table.to_string()
}

pub fn asset_detail(detail: &AssetDetail) -> String {
    let a = &detail.asset;
    let mut lines = vec![
        format!("Asset #{}", a.asset_id),
        format!("  Name:            {}", a.logical_name),
        format!("  Module:          {}", a.module_path),
        format!("  File:            {}", a.file_path),
        format!("  Function:        {}", a.function_name),
        format!("  Definition hash: {}", a.definition_hash),
        format!("  Serializer:      {}", a.serializer_kind),
    ];
    if let Some(rt) = &a.return_type {
        lines.push(format!("  Return type:     {}", rt));
    }
    match &detail.latest_materialization {
        Some(m) => {
            lines.push(format!("  Last job:        #{} ({})", m.materialization_id, m.status));
            if let Some(err) = &m.last_error {
                lines.push(format!("  Error:           {}", err));
            }
        }
        None => {
            lines.push("  Last job:        none".to_string());
        }
    }
    lines.join("\n")
}

pub fn jobs_table(jobs: &[JobDetail]) -> String {
    if jobs.is_empty() {
        return "No jobs found.".to_string();
    }
    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_header(["Job ID", "Asset", "Status", "Run Hash"]);
    for j in jobs {
        let short_hash: String = j.job.run_hash.chars().take(12).collect();
        table.add_row([&j.job.materialization_id.to_string(), &j.asset.function_name, &j.job.status, &short_hash]);
    }
    table.to_string()
}

pub fn job_detail(detail: &JobDetail) -> String {
    let j = &detail.job;
    let a = &detail.asset;
    let mut lines = vec![
        format!("Job #{}", j.materialization_id),
        format!("  Asset:     {} (#{}) ", a.function_name, a.asset_id),
        format!("  Status:    {}", j.status),
        format!("  Run hash:  {}", j.run_hash),
    ];
    if let Some(path) = &j.artifact_path {
        lines.push(format!("  Artifact:  {}", path));
    }
    if let Some(err) = &j.last_error {
        lines.push(format!("  Error:     {}", err));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assets_table_empty() {
        let output = assets_table(&[]);
        assert_eq!(output, "No assets indexed.");
    }

    #[test]
    fn test_assets_table_formatting() {
        let assets = vec![AssetSummary {
            asset_id: 1,
            logical_name: "my_asset".into(),
            module_path: "example.mod".into(),
            file_path: "example/mod.py".into(),
            function_name: "my_func".into(),
            definition_hash: "abc123".into(),
            materialization_status: Some("success".into()),
            materialization_run_hash: Some("abc123".into()),
            materialization_created_at: Some(1700000000),
        }];
        let output = assets_table(&assets);
        assert!(output.contains("my_func"));
        assert!(output.contains("example.mod"));
        assert!(output.contains("success"));
    }

    #[test]
    fn test_jobs_table_empty() {
        let output = jobs_table(&[]);
        assert_eq!(output, "No jobs found.");
    }

    #[test]
    fn test_jobs_table_formatting() {
        use barca_core::models::MaterializationRecord;
        let jobs = vec![JobDetail {
            job: MaterializationRecord {
                materialization_id: 42,
                asset_id: 1,
                definition_id: 1,
                run_hash: "abcdef123456789".into(),
                status: "success".into(),
                artifact_path: None,
                artifact_format: None,
                artifact_checksum: None,
                last_error: None,
                partition_key_json: None,
                created_at: 1700000000,
            },
            asset: AssetSummary {
                asset_id: 1,
                logical_name: "my_asset".into(),
                module_path: "example.mod".into(),
                file_path: "example/mod.py".into(),
                function_name: "my_func".into(),
                definition_hash: "abc123".into(),
                materialization_status: Some("success".into()),
                materialization_run_hash: Some("abc123".into()),
                materialization_created_at: Some(1700000000),
            },
        }];
        let output = jobs_table(&jobs);
        assert!(output.contains("42"));
        assert!(output.contains("my_func"));
        assert!(output.contains("success"));
        assert!(output.contains("abcdef123456"));
    }
}
