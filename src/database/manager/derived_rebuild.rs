/*
 * Light weight and small wiki system for local use
 *
 *  Copyright (C) 2025 Hiroshi KUWAGATA <kgt9221@gmail.com>
 */

//!
//! front matter由来派生データの共通再構成を提供するモジュール
//!

use anyhow::Result;

use super::DatabaseManager;
use super::prompt_candidates::{
    collect_prompt_candidates_in_txn,
    replace_prompt_candidates_in_txn,
};
use super::resource_candidates::{
    collect_resource_candidates_in_txn,
    replace_resource_candidates_in_txn,
};
use super::template_candidates::{
    collect_template_candidates_in_txn,
    replace_template_candidates_in_txn,
};

///
/// 派生データの対象別再構成件数
///
pub(crate) struct DerivedRebuildCounts {
    templates: usize,
    prompts: usize,
    resources: usize,
}

impl DerivedRebuildCounts {
    ///
    /// template候補の再構成件数を返す
    ///
    /// # 戻り値
    /// template候補件数を返す。
    ///
    pub(crate) fn templates(&self) -> usize {
        self.templates
    }

    ///
    /// prompt候補の再構成件数を返す
    ///
    /// # 戻り値
    /// prompt候補件数を返す。
    ///
    pub(crate) fn prompts(&self) -> usize {
        self.prompts
    }

    ///
    /// resource候補の再構成件数を返す
    ///
    /// # 戻り値
    /// resource候補件数を返す。
    ///
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn resources(&self) -> usize {
        self.resources
    }
}

impl DatabaseManager {
    ///
    /// front matter由来の全派生データを再構成する
    ///
    /// # 引数
    /// * `template_root` - legacy template候補取り込み元
    ///
    /// # 戻り値
    /// 対象別の再構成件数を返す。
    ///
    pub(crate) fn rebuild_all_derived_data(
        &self,
        template_root: Option<&str>,
    ) -> Result<DerivedRebuildCounts> {
        let txn = self.db.begin_write()?;

        /*
         * 全対象の候補を収集・検証する
         */
        let template_entries =
            collect_template_candidates_in_txn(
                &txn,
                template_root,
            )?;
        let prompt_data =
            collect_prompt_candidates_in_txn(&txn)?;
        let resource_data =
            collect_resource_candidates_in_txn(&txn)?;
        let counts = DerivedRebuildCounts {
            templates: template_entries.len(),
            prompts: prompt_data.len(),
            resources: resource_data.len(),
        };

        /*
         * 検証済みデータで全対象を置換する
         */
        replace_template_candidates_in_txn(
            &txn,
            &template_entries,
        )?;
        replace_prompt_candidates_in_txn(&txn, &prompt_data)?;
        replace_resource_candidates_in_txn(&txn, &resource_data)?;
        txn.commit()?;

        Ok(counts)
    }
}
