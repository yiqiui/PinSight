//! Localisation.
//!
//! Every user-visible string lives in one table so that adding a language is a
//! matter of adding one column. The table is expanded by [`strings!`] into a
//! plain `struct` of `&'static str` fields, which keeps lookups allocation-free
//! and turns a missing translation into a compile error instead of a runtime
//! surprise.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Lang {
    En,
    ZhHans,
    ZhHant,
    Ja,
    Ko,
    Fr,
    De,
}

impl Default for Lang {
    fn default() -> Self {
        Self::detect()
    }
}

impl Lang {
    pub const ALL: [Lang; 7] = [
        Lang::ZhHans,
        Lang::ZhHant,
        Lang::En,
        Lang::Ja,
        Lang::Ko,
        Lang::Fr,
        Lang::De,
    ];

    /// BCP-47-ish tag, also used as the config value.
    #[allow(dead_code)]
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::ZhHans => "zh-hans",
            Self::ZhHant => "zh-hant",
            Self::Ja => "ja",
            Self::Ko => "ko",
            Self::Fr => "fr",
            Self::De => "de",
        }
    }

    /// Whether the UI language needs CJK glyph coverage in the primary face.
    pub fn needs_cjk(self) -> bool {
        matches!(self, Self::ZhHans | Self::ZhHant | Self::Ja | Self::Ko)
    }

    /// Name of the language **as written in `current`**.
    ///
    /// The picker used to list every language in its own script, which forced
    /// a CJK face (and a Korean face) into memory just to render the list —
    /// egui copies font bytes, so that was ~30 MB for a feature nobody asked
    /// for. Translating the names is what Snipaste does and costs nothing.
    pub fn display_name(self, current: Lang) -> &'static str {
        match current {
            Lang::ZhHans => match self {
                Lang::En => "英语",
                Lang::ZhHans => "简体中文",
                Lang::ZhHant => "繁体中文",
                Lang::Ja => "日语",
                Lang::Ko => "韩语",
                Lang::Fr => "法语",
                Lang::De => "德语",
            },
            Lang::ZhHant => match self {
                Lang::En => "英語",
                Lang::ZhHans => "簡體中文",
                Lang::ZhHant => "繁體中文",
                Lang::Ja => "日語",
                Lang::Ko => "韓語",
                Lang::Fr => "法語",
                Lang::De => "德語",
            },
            Lang::Ja => match self {
                Lang::En => "英語",
                Lang::ZhHans => "簡体字中国語",
                Lang::ZhHant => "繁体字中国語",
                Lang::Ja => "日本語",
                Lang::Ko => "韓国語",
                Lang::Fr => "フランス語",
                Lang::De => "ドイツ語",
            },
            Lang::Ko => match self {
                Lang::En => "영어",
                Lang::ZhHans => "중국어(간체)",
                Lang::ZhHant => "중국어(번체)",
                Lang::Ja => "일본어",
                Lang::Ko => "한국어",
                Lang::Fr => "프랑스어",
                Lang::De => "독일어",
            },
            Lang::Fr => match self {
                Lang::En => "Anglais",
                Lang::ZhHans => "Chinois simplifié",
                Lang::ZhHant => "Chinois traditionnel",
                Lang::Ja => "Japonais",
                Lang::Ko => "Coréen",
                Lang::Fr => "Français",
                Lang::De => "Allemand",
            },
            Lang::De => match self {
                Lang::En => "Englisch",
                Lang::ZhHans => "Vereinfachtes Chinesisch",
                Lang::ZhHant => "Traditionelles Chinesisch",
                Lang::Ja => "Japanisch",
                Lang::Ko => "Koreanisch",
                Lang::Fr => "Französisch",
                Lang::De => "Deutsch",
            },
            Lang::En => match self {
                Lang::En => "English",
                Lang::ZhHans => "Chinese (Simplified)",
                Lang::ZhHant => "Chinese (Traditional)",
                Lang::Ja => "Japanese",
                Lang::Ko => "Korean",
                Lang::Fr => "French",
                Lang::De => "German",
            },
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        let code = code.trim().to_ascii_lowercase().replace('_', "-");
        match code.as_str() {
            "en" | "en-us" | "en-gb" => Some(Self::En),
            "zh" | "zh-cn" | "zh-hans" | "zh-sg" => Some(Self::ZhHans),
            "zh-tw" | "zh-hk" | "zh-mo" | "zh-hant" => Some(Self::ZhHant),
            "ja" | "ja-jp" => Some(Self::Ja),
            "ko" | "ko-kr" => Some(Self::Ko),
            "fr" | "fr-fr" | "fr-ca" => Some(Self::Fr),
            "de" | "de-de" | "de-at" | "de-ch" => Some(Self::De),
            _ => None,
        }
    }

    /// Best-effort match against the operating system UI language.
    pub fn detect() -> Self {
        if let Some(lang) = Self::from_locale(&system_locale()) {
            return lang;
        }
        Self::En
    }

    fn from_locale(locale: &str) -> Option<Self> {
        let normalized = locale.trim().to_ascii_lowercase().replace('_', "-");
        if normalized.is_empty() {
            return None;
        }
        if let Some(lang) = Self::from_code(&normalized) {
            return Some(lang);
        }
        // Fall back to the primary subtag ("de-AT" -> "de").
        normalized.split('-').next().and_then(Self::from_code)
    }
}

#[cfg(windows)]
fn system_locale() -> String {
    use windows::Win32::Globalization::GetUserDefaultUILanguage;
    let langid = unsafe { GetUserDefaultUILanguage() };
    let primary = langid & 0x3ff;
    let sub = langid >> 10;
    match primary {
        0x04 => {
            // Chinese: distinguish Simplified from Traditional by sub-language.
            if matches!(sub, 0x01 | 0x03 | 0x05) {
                "zh-hant".into()
            } else {
                "zh-hans".into()
            }
        }
        0x11 => "ja".into(),
        0x12 => "ko".into(),
        0x0c => "fr".into(),
        0x07 => "de".into(),
        _ => "en".into(),
    }
}

#[cfg(not(windows))]
fn system_locale() -> String {
    std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_else(|_| "en".into())
}

macro_rules! strings {
    ($( $field:ident { en: $en:expr, zh: $zh:expr, tw: $tw:expr, ja: $ja:expr, ko: $ko:expr, fr: $fr:expr, de: $de:expr } )*) => {
        /// All user-visible strings for one language.
        ///
        /// The tray strings are Windows-only, so on other platforms the whole
        /// struct has unused fields by design.
        #[cfg_attr(not(windows), allow(dead_code))]
        #[derive(Debug, Clone, Copy)]
        #[allow(non_snake_case)] // naming codes keep their `$MMMM$` spelling
        pub struct Strings {
            $(pub $field: &'static str,)*
        }

        impl Strings {
            pub fn get(lang: Lang) -> &'static Self {
                match lang {
                    Lang::En => &EN,
                    Lang::ZhHans => &ZH_HANS,
                    Lang::ZhHant => &ZH_HANT,
                    Lang::Ja => &JA,
                    Lang::Ko => &KO,
                    Lang::Fr => &FR,
                    Lang::De => &DE,
                }
            }
        }

        static EN: Strings = Strings { $($field: $en,)* };
        static ZH_HANS: Strings = Strings { $($field: $zh,)* };
        static ZH_HANT: Strings = Strings { $($field: $tw,)* };
        static JA: Strings = Strings { $($field: $ja,)* };
        static KO: Strings = Strings { $($field: $ko,)* };
        static FR: Strings = Strings { $($field: $fr,)* };
        static DE: Strings = Strings { $($field: $de,)* };
    };
}

strings! {
    // ---- chrome -----------------------------------------------------------
    tagline { en: "native capture · pin · record", zh: "原生截图 · 贴图 · 录屏", tw: "原生截圖 · 貼圖 · 錄影", ja: "ネイティブ撮影 · ピン · 録画", ko: "네이티브 캡처 · 핀 · 녹화", fr: "capture · épinglage · enregistrement natifs", de: "Nativ aufnehmen · anheften · aufzeichnen" }
    // Shown once at startup when the on-disk `config.json` could not be parsed:
    // the app reset to defaults and kept the bad file aside. `{path}` is the
    // location the malformed file was moved to (a `config.json.corrupt-<ts>`).
    cfg_reset { en: "Settings file was corrupt and reset to defaults. Backup kept at {path}", zh: "配置文件已损坏，已恢复为默认设置。原文件已另存为 {path}", tw: "設定檔已損壞，已恢復為預設設定。原檔案已另存為 {path}", ja: "設定ファイルが破損していたため既定値に戻しました。バックアップを {path} に保存しました", ko: "설정 파일이 손상되어 기본값으로 초기화되었습니다. 백업이 {path}에 저장되었습니다", fr: "Le fichier de configuration était corrompu et a été réinitialisé aux valeurs par défaut. Une copie est conservée dans {path}", de: "Die Konfigurationsdatei war beschädigt und wurde auf die Standardwerte zurückgesetzt. Eine Kopie liegt in {path}" }
    nav_capture { en: "Capture", zh: "截图", tw: "截圖", ja: "キャプチャ", ko: "캡처", fr: "Capturer", de: "Aufnehmen" }
    nav_pins { en: "Pins", zh: "贴图", tw: "貼圖", ja: "ピン", ko: "핀", fr: "Épingles", de: "Angeheftet" }
    nav_history { en: "History", zh: "图库", tw: "圖庫", ja: "履歴", ko: "기록", fr: "Historique", de: "Verlauf" }
    history_title { en: "Capture history", zh: "截图历史", tw: "截圖歷史", ja: "キャプチャ履歴", ko: "캡처 기록", fr: "Historique des captures", de: "Aufnahmeverlauf" }
    history_subtitle { en: "Everything saved to your capture folder", zh: "保存目录里的所有截图", tw: "儲存目錄裡的所有截圖", ja: "保存フォルダ内のすべてのキャプチャ", ko: "저장 폴더의 모든 캡처", fr: "Toutes les captures du dossier", de: "Alle Aufnahmen im Speicherordner" }
    history_empty { en: "No captures yet", zh: "还没有截图", tw: "還沒有截圖", ja: "キャプチャはまだありません", ko: "아직 캡처가 없습니다", fr: "Aucune capture pour l'instant", de: "Noch keine Aufnahmen" }
    history_pin { en: "Pin to screen", zh: "贴到屏幕", tw: "貼到螢幕", ja: "画面に固定", ko: "화면에 고정", fr: "Épingler", de: "Anheften" }
    history_delete { en: "Delete", zh: "删除", tw: "刪除", ja: "削除", ko: "삭제", fr: "Supprimer", de: "Löschen" }
    history_select_all { en: "Select all", zh: "全选", tw: "全選", ja: "すべて選択", ko: "전체 선택", fr: "Tout sélectionner", de: "Alle auswählen" }
    history_clear_sel { en: "Clear", zh: "清除选择", tw: "清除選擇", ja: "選択を解除", ko: "선택 해제", fr: "Effacer", de: "Auswahl löschen" }
    history_export_pdf { en: "Export PDF", zh: "导出 PDF", tw: "匯出 PDF", ja: "PDF に書き出し", ko: "PDF 내보내기", fr: "Exporter PDF", de: "PDF exportieren" }
    history_selected { en: "selected", zh: "已选", tw: "已選", ja: "選択中", ko: "선택됨", fr: "sélectionnés", de: "ausgewählt" }
    history_delete_sel { en: "Delete selected", zh: "删除所选", tw: "刪除所選", ja: "選択を削除", ko: "선택 항목 삭제", fr: "Supprimer la sélection", de: "Auswahl löschen" }
    history_delete_confirm { en: "Press again to delete for good", zh: "再按一次确认删除", tw: "再按一次確認刪除", ja: "もう一度押すと完全に削除", ko: "한 번 더 누르면 완전히 삭제", fr: "Appuyez encore pour supprimer", de: "Erneut drücken zum Löschen" }
    history_deleted { en: "deleted", zh: "张已删除", tw: "張已刪除", ja: "件を削除しました", ko: "개 삭제됨", fr: "supprimés", de: "gelöscht" }
    history_delete_failed { en: "failed", zh: "张删除失败", tw: "張刪除失敗", ja: "件削除に失敗", ko: "개 삭제 실패", fr: "échecs", de: "fehlgeschlagen" }
    history_exported { en: "images exported to PDF", zh: "张图片已导出为 PDF", tw: "張圖片已匯出為 PDF", ja: "枚を PDF に書き出し", ko: "개 이미지 PDF 내보냄", fr: "images exportées en PDF", de: "Bilder als PDF exportiert" }
    history_export_busy { en: "A PDF export is still running", zh: "上一次 PDF 导出仍在进行中", tw: "上一次 PDF 匯出仍在進行中", ja: "前回の PDF 書き出しが進行中です", ko: "이전 PDF 내보내기가 아직 진행 중입니다", fr: "Un export PDF est encore en cours", de: "Ein PDF-Export läuft noch" }
    nav_settings { en: "Settings", zh: "设置", tw: "設定", ja: "設定", ko: "설정", fr: "Réglages", de: "Einstellungen" }
    mcp_on { en: "MCP on", zh: "MCP 已启用", tw: "MCP 已啟用", ja: "MCP 有効", ko: "MCP 켜짐", fr: "MCP actif", de: "MCP aktiv" }
    mcp_off { en: "MCP off", zh: "MCP 未启用", tw: "MCP 未啟用", ja: "MCP 無効", ko: "MCP 꺼짐", fr: "MCP inactif", de: "MCP aus" }

    // ---- capture page -----------------------------------------------------
    capture_title { en: "Capture", zh: "截图", tw: "截圖", ja: "キャプチャ", ko: "캡처", fr: "Capturer", de: "Aufnehmen" }
    capture_subtitle { en: "Pick a region, the whole screen, or start a scroll capture.", zh: "框选区域、截取全屏，或开始滚动长截图。", tw: "框選區域、截取全螢幕，或開始捲動長截圖。", ja: "範囲・全画面を撮るか、スクロール撮影を開始します。", ko: "영역이나 전체 화면을 캡처하거나 스크롤 캡처를 시작합니다.", fr: "Choisissez une zone, tout l'écran, ou lancez une capture défilante.", de: "Bereich, ganzen Bildschirm oder Scroll-Aufnahme starten." }
    act_area_title { en: "Capture area", zh: "区域截图", tw: "區域截圖", ja: "範囲を撮影", ko: "영역 캡처", fr: "Capturer une zone", de: "Bereich aufnehmen" }
    act_area_desc { en: "Freeze the screen and drag a selection", zh: "冻结屏幕后拖拽框选", tw: "凍結螢幕後拖曳框選", ja: "画面を固定して範囲を選択", ko: "화면을 고정하고 영역을 선택", fr: "Figer l'écran puis sélectionner", de: "Bildschirm einfrieren und auswählen" }
    act_window_title { en: "Capture window", zh: "窗口截图", tw: "視窗截圖", ja: "ウィンドウを撮影", ko: "창 캡처", fr: "Capturer une fenêtre", de: "Fenster aufnehmen" }
    act_window_desc { en: "Pick a top-level window to capture", zh: "从列表中选择一个窗口", tw: "從清單中選擇一個視窗", ja: "一覧からウィンドウを選択", ko: "목록에서 창을 선택", fr: "Choisir une fenêtre", de: "Ein Fenster auswählen" }
    window_picker_title { en: "Pick a window", zh: "选择窗口", tw: "選擇視窗", ja: "ウィンドウを選択", ko: "창 선택", fr: "Choisir une fenêtre", de: "Fenster wählen" }
    window_picker_empty { en: "No capturable windows found", zh: "没有可截取的窗口", tw: "沒有可截取的視窗", ja: "撮影できるウィンドウがありません", ko: "캡처할 창이 없습니다", fr: "Aucune fenêtre capturable", de: "Keine aufnehmbaren Fenster" }
    act_full_title { en: "Full screen", zh: "全屏截图", tw: "全螢幕截圖", ja: "全画面", ko: "전체 화면", fr: "Plein écran", de: "Ganzer Bildschirm" }
    act_full_desc { en: "Copy the display under the cursor", zh: "截取光标所在显示器并复制", tw: "截取游標所在螢幕並複製", ja: "カーソル下のディスプレイをコピー", ko: "커서가 있는 모니터를 복사", fr: "Copier l'écran sous le curseur", de: "Bildschirm unter dem Cursor kopieren" }
    act_pin_title { en: "Pin clipboard", zh: "贴图剪贴板", tw: "貼圖剪貼簿", ja: "クリップボードを固定", ko: "클립보드 고정", fr: "Épingler le presse-papiers", de: "Zwischenablage anheften" }
    act_pin_desc { en: "Float the clipboard image on top", zh: "把剪贴板图片钉在屏幕最上层", tw: "把剪貼簿圖片釘在最上層", ja: "クリップボード画像を最前面に表示", ko: "클립보드 이미지를 항상 위에 표시", fr: "Afficher l'image du presse-papiers au premier plan", de: "Bild aus der Zwischenablage schwebend anzeigen" }
    act_long_title { en: "Scroll capture", zh: "长截图", tw: "長截圖", ja: "スクロール撮影", ko: "스크롤 캡처", fr: "Capture défilante", de: "Scroll-Aufnahme" }
    act_long_desc { en: "Auto-scroll and stitch frames together", zh: "自动滚动并拼接多帧画面", tw: "自動捲動並拼接多幀畫面", ja: "自動スクロールしてフレームを結合", ko: "자동 스크롤 후 프레임을 이어 붙임", fr: "Défilement automatique et assemblage", de: "Automatisch scrollen und Bilder verbinden" }
    act_record_title { en: "Record area", zh: "区域录屏", tw: "區域錄影", ja: "範囲を録画", ko: "영역 녹화", fr: "Enregistrer une zone", de: "Bereich aufzeichnen" }
    act_record_desc { en: "Capture frames to MP4 or GIF", zh: "逐帧录制为 MP4 或 GIF", tw: "逐幀錄製為 MP4 或 GIF", ja: "フレームを MP4 / GIF に保存", ko: "프레임을 MP4 또는 GIF로 저장", fr: "Enregistrer en MP4 ou GIF", de: "Frames als MP4 oder GIF speichern" }
    act_save_title { en: "Save last", zh: "保存最近截图", tw: "儲存最近截圖", ja: "直前の撮影を保存", ko: "최근 캡처 저장", fr: "Enregistrer la dernière", de: "Letzte Aufnahme sichern" }
    act_save_desc { en: "Write the last capture to disk", zh: "把最近一次截图写入磁盘", tw: "把最近一次截圖寫入磁碟", ja: "直前の撮影をファイルに保存", ko: "최근 캡처를 파일로 저장", fr: "Écrire la dernière capture sur le disque", de: "Letzte Aufnahme auf der Festplatte speichern" }
    recent_title { en: "Last capture", zh: "最近截图", tw: "最近截圖", ja: "直前の撮影", ko: "최근 캡처", fr: "Dernière capture", de: "Letzte Aufnahme" }
    recent_empty { en: "Nothing captured yet. Press the area shortcut to begin.", zh: "还没有截图。按区域截图快捷键开始。", tw: "還沒有截圖。按區域截圖快速鍵開始。", ja: "まだ撮影がありません。範囲撮影のショートカットで開始します。", ko: "아직 캡처가 없습니다. 영역 캡처 단축키로 시작하세요.", fr: "Aucune capture pour l'instant. Utilisez le raccourci de zone.", de: "Noch nichts aufgenommen. Bereichs-Shortcut drücken." }
    activity_title { en: "Running", zh: "进行中", tw: "進行中", ja: "実行中", ko: "진행 중", fr: "En cours", de: "Läuft" }
    stop_recording { en: "Stop", zh: "停止", tw: "停止", ja: "停止", ko: "중지", fr: "Arrêter", de: "Stoppen" }
    frames { en: "frames", zh: "帧", tw: "幀", ja: "フレーム", ko: "프레임", fr: "images", de: "Frames" }
    long_running { en: "Scroll capture in progress", zh: "正在生成长截图", tw: "正在產生長截圖", ja: "スクロール撮影中", ko: "스크롤 캡처 진행 중", fr: "Capture défilante en cours", de: "Scroll-Aufnahme läuft" }
    tall { en: "tall", zh: "高", tw: "高", ja: "高さ", ko: "높이", fr: "hauteur", de: "Höhe" }

    // ---- pins page --------------------------------------------------------
    pins_title { en: "Pins", zh: "贴图", tw: "貼圖", ja: "ピン", ko: "핀", fr: "Épingles", de: "Angeheftet" }
    pins_subtitle { en: "Pinned images float above every window.", zh: "贴图会浮在所有窗口之上。", tw: "貼圖會浮在所有視窗之上。", ja: "ピンした画像はすべてのウィンドウの前面に表示されます。", ko: "고정한 이미지는 모든 창 위에 표시됩니다.", fr: "Les images épinglées flottent au-dessus des fenêtres.", de: "Angeheftete Bilder schweben über allen Fenstern." }
    pins_empty_title { en: "No pins yet", zh: "还没有贴图", tw: "還沒有貼圖", ja: "ピンはまだありません", ko: "아직 핀이 없습니다", fr: "Aucune épingle", de: "Noch nichts angeheftet" }
    pins_empty_desc { en: "Capture a region and press P, or pin the clipboard.", zh: "截图后按 P 贴图，或直接贴图剪贴板。", tw: "截圖後按 P 貼圖，或直接貼圖剪貼簿。", ja: "撮影後に P でピン、またはクリップボードを固定します。", ko: "캡처 후 P를 누르거나 클립보드를 고정하세요.", fr: "Capturez puis appuyez sur P, ou épinglez le presse-papiers.", de: "Bereich aufnehmen und P drücken oder Zwischenablage anheften." }
    close_all { en: "Close all", zh: "全部关闭", tw: "全部關閉", ja: "すべて閉じる", ko: "모두 닫기", fr: "Tout fermer", de: "Alle schließen" }
    pin_focus { en: "Focus", zh: "聚焦", tw: "聚焦", ja: "前面へ", ko: "앞으로", fr: "Focus", de: "Fokus" }
    pin_through { en: "Click-through", zh: "点击穿透", tw: "點擊穿透", ja: "クリック透過", ko: "클릭 통과", fr: "Clic traversant", de: "Klick durch" }
    pin_copy { en: "Copy", zh: "复制", tw: "複製", ja: "コピー", ko: "복사", fr: "Copier", de: "Kopieren" }
    pin_close { en: "Close", zh: "关闭", tw: "關閉", ja: "閉じる", ko: "닫기", fr: "Fermer", de: "Schließen" }
    pin_menu_copy { en: "Copy image", zh: "复制图像", tw: "複製圖像", ja: "画像をコピー", ko: "이미지 복사", fr: "Copier l'image", de: "Bild kopieren" }
    pin_menu_save_as { en: "Save as...", zh: "图像另存为...", tw: "圖像另存為...", ja: "名前を付けて保存...", ko: "다른 이름으로 저장...", fr: "Enregistrer sous...", de: "Speichern unter..." }
    pin_menu_zoom { en: "Zoom", zh: "缩放", tw: "縮放", ja: "拡大縮小", ko: "확대/축소", fr: "Zoom", de: "Zoom" }
    pin_menu_ontop { en: "Always on top", zh: "总是置顶", tw: "總是置頂", ja: "常に前面表示", ko: "항상 위", fr: "Toujours devant", de: "Immer im Vordergrund" }
    pin_menu_through { en: "Click-through", zh: "点击穿透", tw: "點擊穿透", ja: "クリック透過", ko: "클릭 통과", fr: "Clic traversant", de: "Klick durch" }
    pin_menu_close { en: "Close", zh: "关闭", tw: "關閉", ja: "閉じる", ko: "닫기", fr: "Fermer", de: "Schließen" }
    pin_menu_destroy { en: "Destroy", zh: "销毁", tw: "銷毀", ja: "破棄", ko: "파괴", fr: "Détruire", de: "Zerstören" }
    pin_saved { en: "Image saved", zh: "图像已保存", tw: "圖像已儲存", ja: "画像を保存しました", ko: "이미지 저장됨", fr: "Image enregistrée", de: "Bild gespeichert" }

    // ---- settings ---------------------------------------------------------
    settings_title { en: "Settings", zh: "设置", tw: "設定", ja: "設定", ko: "설정", fr: "Réglages", de: "Einstellungen" }
    settings_subtitle { en: "Appearance, output and hotkeys. Changes apply instantly.", zh: "外观、输出与快捷键，修改即时生效。", tw: "外觀、輸出與快速鍵，修改即時生效。", ja: "外観・出力・ショートカット。変更は即時反映されます。", ko: "모양, 출력, 단축키. 변경 사항은 즉시 적용됩니다.", fr: "Apparence, sortie et raccourcis. Application immédiate.", de: "Aussehen, Ausgabe und Tastenkürzel. Änderungen sofort aktiv." }
    sec_appearance { en: "Appearance", zh: "外观", tw: "外觀", ja: "外観", ko: "모양", fr: "Apparence", de: "Aussehen" }
    sec_output { en: "Output", zh: "输出", tw: "輸出", ja: "出力", ko: "출력", fr: "Sortie", de: "Ausgabe" }
    sec_hotkeys { en: "Hotkeys", zh: "快捷键", tw: "快速鍵", ja: "ショートカット", ko: "단축키", fr: "Raccourcis", de: "Tastenkürzel" }
    sec_about { en: "Status", zh: "状态", tw: "狀態", ja: "ステータス", ko: "상태", fr: "État", de: "Status" }
    theme { en: "Theme", zh: "主题", tw: "主題", ja: "テーマ", ko: "테마", fr: "Thème", de: "Design" }
    theme_system { en: "System", zh: "跟随系统", tw: "跟隨系統", ja: "システム", ko: "시스템", fr: "Système", de: "System" }
    theme_light { en: "Light", zh: "浅色", tw: "淺色", ja: "ライト", ko: "라이트", fr: "Clair", de: "Hell" }
    theme_dark { en: "Dark", zh: "深色", tw: "深色", ja: "ダーク", ko: "다크", fr: "Sombre", de: "Dunkel" }
    accent { en: "Accent", zh: "强调色", tw: "強調色", ja: "アクセント", ko: "강조 색", fr: "Accent", de: "Akzentfarbe" }
    accent_blue { en: "Blue", zh: "蓝色", tw: "藍色", ja: "ブルー", ko: "파랑", fr: "Bleu", de: "Blau" }
    accent_violet { en: "Violet", zh: "紫色", tw: "紫色", ja: "バイオレット", ko: "보라", fr: "Violet", de: "Violett" }
    accent_emerald { en: "Emerald", zh: "翠绿", tw: "翠綠", ja: "エメラルド", ko: "에메랄드", fr: "Émeraude", de: "Smaragd" }
    accent_amber { en: "Amber", zh: "琥珀", tw: "琥珀", ja: "アンバー", ko: "앰버", fr: "Ambre", de: "Bernstein" }
    accent_rose { en: "Rose", zh: "玫瑰", tw: "玫瑰", ja: "ローズ", ko: "로즈", fr: "Rose", de: "Rosé" }
    accent_cyan { en: "Cyan", zh: "青色", tw: "青色", ja: "シアン", ko: "시안", fr: "Cyan", de: "Cyan" }
    language { en: "Language", zh: "语言", tw: "語言", ja: "言語", ko: "언어", fr: "Langue", de: "Sprache" }
    font_family { en: "Font", zh: "字体", tw: "字型", ja: "フォント", ko: "글꼴", fr: "Police", de: "Schrift" }
    font_auto { en: "Auto (match language)", zh: "自动（匹配语言）", tw: "自動（符合語言）", ja: "自動（言語に合わせる）", ko: "자동 (언어에 맞춤)", fr: "Auto (selon la langue)", de: "Automatisch (nach Sprache)" }
    font_size { en: "Text size", zh: "字号", tw: "字級", ja: "文字サイズ", ko: "글자 크기", fr: "Taille du texte", de: "Textgröße" }
    font_path_hint { en: "Path to a .ttf / .otf / .ttc file", zh: ".ttf / .otf / .ttc 文件路径", tw: ".ttf / .otf / .ttc 檔案路徑", ja: ".ttf / .otf / .ttc ファイルのパス", ko: ".ttf / .otf / .ttc 파일 경로", fr: "Chemin d'un fichier .ttf / .otf / .ttc", de: "Pfad zu einer .ttf / .otf / .ttc-Datei" }
    reset { en: "Reset", zh: "恢复默认", tw: "回復預設", ja: "リセット", ko: "초기화", fr: "Réinitialiser", de: "Zurücksetzen" }
    output_dir { en: "Save folder", zh: "保存目录", tw: "儲存目錄", ja: "保存先", ko: "저장 폴더", fr: "Dossier de sortie", de: "Speicherordner" }
    open_folder { en: "Open folder", zh: "打开目录", tw: "開啟目錄", ja: "フォルダーを開く", ko: "폴더 열기", fr: "Ouvrir le dossier", de: "Ordner öffnen" }
    config_file { en: "Config file", zh: "配置文件", tw: "設定檔", ja: "設定ファイル", ko: "설정 파일", fr: "Fichier de config", de: "Konfigurationsdatei" }
    open_config { en: "Reveal", zh: "定位文件", tw: "定位檔案", ja: "場所を開く", ko: "위치 열기", fr: "Afficher", de: "Anzeigen" }
    apply { en: "Apply", zh: "应用", tw: "套用", ja: "適用", ko: "적용", fr: "Appliquer", de: "Anwenden" }
    hotkeys_hint { en: "Use Ctrl / Alt / Shift / Win plus one key. Re-registering applies at once.", zh: "使用 Ctrl / Alt / Shift / Win 加一个按键，修改后立即重新注册。", tw: "使用 Ctrl / Alt / Shift / Win 加一個按鍵，修改後立即重新註冊。", ja: "Ctrl / Alt / Shift / Win と 1 つのキーを組み合わせます。変更は即時反映されます。", ko: "Ctrl / Alt / Shift / Win 과 키 하나를 조합하세요. 변경 즉시 다시 등록됩니다.", fr: "Ctrl / Alt / Shift / Win plus une touche. Réenregistrement immédiat.", de: "Ctrl / Alt / Shift / Win plus eine Taste. Sofort neu registriert." }
    hk_capture_area { en: "Capture area", zh: "区域截图", tw: "區域截圖", ja: "範囲を撮影", ko: "영역 캡처", fr: "Capturer une zone", de: "Bereich aufnehmen" }
    hk_capture_full { en: "Full screen", zh: "全屏截图", tw: "全螢幕截圖", ja: "全画面", ko: "전체 화면", fr: "Plein écran", de: "Ganzer Bildschirm" }
    hk_pin_clipboard { en: "Pin clipboard", zh: "贴图剪贴板", tw: "貼圖剪貼簿", ja: "クリップボード固定", ko: "클립보드 고정", fr: "Épingler presse-papiers", de: "Zwischenablage" }
    hk_long_capture { en: "Scroll capture", zh: "长截图", tw: "長截圖", ja: "スクロール撮影", ko: "스크롤 캡처", fr: "Capture défilante", de: "Scroll-Aufnahme" }
    hk_record { en: "Record / stop", zh: "录屏 / 停止", tw: "錄影 / 停止", ja: "録画 / 停止", ko: "녹화 / 중지", fr: "Enregistrer / arrêter", de: "Aufzeichnen / Stopp" }
    hk_save_last { en: "Save last", zh: "保存最近", tw: "儲存最近", ja: "直前を保存", ko: "최근 저장", fr: "Enregistrer la dernière", de: "Letzte sichern" }
    hk_toggle_pins { en: "Show / hide pins", zh: "显示 / 隐藏贴图", tw: "顯示 / 隱藏貼圖", ja: "ピン表示切替", ko: "핀 표시 전환", fr: "Afficher / masquer les épingles", de: "Pins ein-/ausblenden" }
    hk_repeat_capture { en: "Repeat last area", zh: "重复上次区域", tw: "重複上次區域", ja: "前回の範囲を再取得", ko: "마지막 영역 반복", fr: "Répéter la dernière zone", de: "Letzten Bereich wiederholen" }
    hk_pick_color { en: "Colour picker", zh: "取色", tw: "取色", ja: "カラー選択", ko: "색상 추출", fr: "Pipette de couleur", de: "Farbpipette" }
    hk_group_hotkey { en: "Group hotkey", zh: "分组热键", tw: "分組熱鍵", ja: "グループ ホットキー", ko: "그룹 단축키", fr: "Raccourci du groupe", de: "Gruppen-Kürzel" }
    tray_repeat_last { en: "Repeat last area", zh: "重复上次区域", tw: "重複上次區域", ja: "前回の範囲を再取得", ko: "마지막 영역 반복", fr: "Répéter la dernière zone", de: "Letzten Bereich wiederholen" }
    tray_repeat_copy { en: "Copy", zh: "复制", tw: "複製", ja: "コピー", ko: "복사", fr: "Copier", de: "Kopieren" }
    tray_repeat_save { en: "Save to file", zh: "保存到文件", tw: "儲存到檔案", ja: "ファイルに保存", ko: "파일로 저장", fr: "Enregistrer", de: "Speichern" }
    tray_repeat_pin { en: "Pin to screen", zh: "贴到屏幕", tw: "貼到螢幕", ja: "画面にピン留め", ko: "화면에 고정", fr: "Épingler", de: "Anheften" }
    version { en: "Version", zh: "版本", tw: "版本", ja: "バージョン", ko: "버전", fr: "Version", de: "Version" }
    mcp_status { en: "MCP server", zh: "MCP 服务", tw: "MCP 服務", ja: "MCP サーバー", ko: "MCP 서버", fr: "Serveur MCP", de: "MCP-Server" }
    mcp_desc_on { en: "Listening on stdio — AI clients can drive PinSight.", zh: "正在 stdio 上监听，AI 客户端可调用 PinSight。", tw: "正在 stdio 上監聽，AI 用戶端可呼叫 PinSight。", ja: "stdio で待機中。AI クライアントから操作できます。", ko: "stdio에서 대기 중. AI 클라이언트가 PinSight를 제어할 수 있습니다.", fr: "En écoute sur stdio — les clients IA peuvent piloter PinSight.", de: "Lauscht auf stdio — KI-Clients können PinSight steuern." }
    mcp_desc_off { en: "Disabled. Restart with --mcp to expose the tools.", zh: "未启用。使用 --mcp 重启以暴露工具。", tw: "未啟用。使用 --mcp 重啟以提供工具。", ja: "無効です。--mcp 付きで再起動するとツールを公開します。", ko: "꺼져 있습니다. --mcp로 다시 시작하면 도구가 노출됩니다.", fr: "Désactivé. Relancez avec --mcp pour exposer les outils.", de: "Deaktiviert. Mit --mcp neu starten, um Werkzeuge anzubieten." }
    displays { en: "Displays", zh: "显示器", tw: "螢幕", ja: "ディスプレイ", ko: "모니터", fr: "Écrans", de: "Bildschirme" }
    record_format { en: "Recording", zh: "录制格式", tw: "錄影格式", ja: "録画形式", ko: "녹화 형식", fr: "Enregistrement", de: "Aufzeichnung" }
    record_mp4 { en: "MP4 via ffmpeg (GIF fallback)", zh: "MP4（通过 ffmpeg，否则 GIF）", tw: "MP4（透過 ffmpeg，否則 GIF）", ja: "ffmpeg で MP4（なければ GIF）", ko: "ffmpeg로 MP4 (없으면 GIF)", fr: "MP4 via ffmpeg (sinon GIF)", de: "MP4 über ffmpeg (sonst GIF)" }
    record_gif { en: "GIF — install ffmpeg for MP4", zh: "GIF — 安装 ffmpeg 可输出 MP4", tw: "GIF — 安裝 ffmpeg 可輸出 MP4", ja: "GIF — ffmpeg を入れると MP4", ko: "GIF — ffmpeg 설치 시 MP4", fr: "GIF — installez ffmpeg pour MP4", de: "GIF — ffmpeg installieren für MP4" }
    hotkeys_registered { en: "registered", zh: "已注册", tw: "已註冊", ja: "登録済み", ko: "등록됨", fr: "enregistrés", de: "registriert" }
    portable_mode { en: "Install mode", zh: "安装模式", tw: "安裝模式", ja: "インストール形態", ko: "설치 방식", fr: "Mode d'installation", de: "Installationsart" }
    portable_installed { en: "Installed (settings in %APPDATA%)", zh: "已安装（配置在 %APPDATA%）", tw: "已安裝（設定在 %APPDATA%）", ja: "インストール版（設定は %APPDATA%）", ko: "설치됨 (설정은 %APPDATA%)", fr: "Installée (réglages dans %APPDATA%)", de: "Installiert (Einstellungen in %APPDATA%)" }
    portable_yes { en: "Portable (everything next to the .exe)", zh: "便携版（所有文件都在 exe 旁边）", tw: "便攜版（所有檔案都在 exe 旁邊）", ja: "ポータブル版（すべて exe の隣）", ko: "포터블 (모든 파일이 exe 옆)", fr: "Portable (tout à côté du .exe)", de: "Portabel (alles neben der .exe)" }
    config_installed_hint { en: "Restart with --portable, or drop a pinsight.portable file next to the .exe, to switch.", zh: "用 --portable 启动，或在 exe 旁放一个 pinsight.portable 文件即可切换。", tw: "以 --portable 啟動，或在 exe 旁放一個 pinsight.portable 檔案即可切換。", ja: "--portable で起動、または exe の隣に pinsight.portable を置くと切り替わります。", ko: "--portable로 시작하거나 exe 옆에 pinsight.portable 파일을 두면 전환됩니다.", fr: "Relancez avec --portable, ou posez un fichier pinsight.portable à côté du .exe.", de: "Mit --portable starten oder eine pinsight.portable-Datei neben die .exe legen." }
    overlay_help { en: "Overlay: drag to select · V R E A D H M tools · Enter copy · P pin · Ctrl+S save · Esc cancel", zh: "浮层：拖拽选区 · V R E A D H M 切换工具 · Enter 复制 · P 贴图 · Ctrl+S 保存 · Esc 取消", tw: "浮層：拖曳選區 · V R E A D H M 切換工具 · Enter 複製 · P 貼圖 · Ctrl+S 儲存 · Esc 取消", ja: "オーバーレイ：ドラッグで選択 · V R E A D H M ツール · Enter コピー · P ピン · Ctrl+S 保存 · Esc 中止", ko: "오버레이: 드래그로 선택 · V R E A D H M 도구 · Enter 복사 · P 고정 · Ctrl+S 저장 · Esc 취소", fr: "Superposition : glisser pour sélectionner · outils V R E A D H M · Entrée copier · P épingler · Ctrl+S enregistrer · Échap annuler", de: "Overlay: ziehen zum Auswählen · Werkzeuge V R E A D H M · Enter kopieren · P anheften · Strg+S speichern · Esc abbrechen" }

    // ---- settings: tabs, background, window ------------------------------
    tab_general { en: "General", zh: "常规", tw: "一般", ja: "全般", ko: "일반", fr: "Général", de: "Allgemein" }
    tab_capture { en: "Capture", zh: "截屏", tw: "截圖", ja: "キャプチャ", ko: "캡처", fr: "Capture", de: "Aufnahme" }
    tab_pins { en: "Pins", zh: "贴图", tw: "貼圖", ja: "ピン", ko: "핀", fr: "Épingles", de: "Angeheftet" }
    tab_controls { en: "Controls", zh: "控制", tw: "控制", ja: "操作", ko: "조작", fr: "Contrôles", de: "Steuerung" }
    tab_updates { en: "Updates", zh: "更新", tw: "更新", ja: "更新", ko: "업데이트", fr: "Mises à jour", de: "Updates" }
    sec_capture { en: "Capture", zh: "截屏", tw: "截圖", ja: "キャプチャ", ko: "캡처", fr: "Capture", de: "Aufnahme" }
    sec_pins { en: "Pins", zh: "贴图", tw: "貼圖", ja: "ピン", ko: "핀", fr: "Épingles", de: "Angeheftet" }
    sec_controls { en: "Controls", zh: "控制", tw: "控制", ja: "操作", ko: "조작", fr: "Contrôles", de: "Steuerung" }
    sec_updates { en: "Updates", zh: "更新", tw: "更新", ja: "更新", ko: "업데이트", fr: "Mises à jour", de: "Updates" }
    cap_border { en: "Border width", zh: "边框宽度", tw: "邊框寬度", ja: "枠の太さ", ko: "테두리 두께", fr: "Épaisseur du cadre", de: "Rahmenbreite" }
    cap_mask { en: "Mask opacity", zh: "遮罩不透明度", tw: "遮罩不透明度", ja: "マスクの不透明度", ko: "마스크 불투명도", fr: "Opacité du masque", de: "Maskendeckkraft" }
    cap_anchors { en: "Show anchors", zh: "显示锚点", tw: "顯示錨點", ja: "アンカーを表示", ko: "앵커 표시", fr: "Afficher les ancres", de: "Anker zeigen" }
    cap_crosshair { en: "Full-screen crosshair", zh: "显示全屏十字线", tw: "顯示全螢幕十字線", ja: "全画面十字線", ko: "전체 화면 십자선", fr: "Réticule plein écran", de: "Fadenkreuz" }
    cap_hints { en: "Show shortcut hints", zh: "显示快捷键提示", tw: "顯示快捷鍵提示", ja: "ショートカットのヒント", ko: "단축키 안내 표시", fr: "Afficher les raccourcis", de: "Tastenkürzel anzeigen" }
    cap_mask_color { en: "Mask colour", zh: "遮罩颜色", tw: "遮罩顏色", ja: "マスクの色", ko: "마스크 색상", fr: "Couleur du masque", de: "Maskenfarbe" }
    cap_anchor_color { en: "Anchor outline", zh: "锚点描边颜色", tw: "錨點描邊顏色", ja: "アンカーの枠色", ko: "앵커 테두리 색", fr: "Contour des ancres", de: "Ankerumriss" }
    cap_behavior { en: "Behaviour", zh: "行为", tw: "行為", ja: "動作", ko: "동작", fr: "Comportement", de: "Verhalten" }
    cap_history_limit { en: "History entries", zh: "最大历史记录数", tw: "最大歷史記錄數", ja: "履歴の最大件数", ko: "최대 기록 수", fr: "Entrées d'historique", de: "Verlaufseinträge" }
    cap_delay { en: "Capture delay", zh: "延时截图", tw: "延時截圖", ja: "撮影の遅延", ko: "캡처 지연", fr: "Délai de capture", de: "Aufnahmeverzögerung" }
    cap_delay_unit { en: "s — capturing…", zh: "秒后截图…", tw: "秒後截圖…", ja: "秒後に撮影…", ko: "초 후 캡처…", fr: "s — capture…", de: "s — Aufnahme…" }
    cap_keys { en: "Keys", zh: "按键", tw: "按鍵", ja: "キー", ko: "키", fr: "Touches", de: "Tasten" }
    cap_esc_confirm { en: "Ask before Esc discards the capture", zh: "按 Esc 时弹出确认对话框", tw: "按 Esc 時彈出確認對話框", ja: "Esc で確認ダイアログを出す", ko: "Esc 시 확인 대화상자 표시", fr: "Confirmer avant d'abandonner avec Échap", de: "Vor Esc-Verwerfen nachfragen" }
    cap_double_click { en: "Left double-click", zh: "左键双击", tw: "左鍵雙擊", ja: "左ダブルクリック", ko: "왼쪽 더블 클릭", fr: "Double-clic gauche", de: "Doppelklick links" }
    cap_middle_click { en: "Middle click", zh: "中键单击", tw: "中鍵單擊", ja: "中クリック", ko: "가운데 클릭", fr: "Clic du milieu", de: "Mittelklick" }
    click_copy { en: "Copy", zh: "复制到剪贴板", tw: "複製到剪貼簿", ja: "コピー", ko: "복사", fr: "Copier", de: "Kopieren" }
    click_pin { en: "Pin to screen", zh: "贴到屏幕", tw: "貼到螢幕", ja: "画面に固定", ko: "화면에 고정", fr: "Épingler", de: "Anheften" }
    click_save { en: "Save", zh: "保存", tw: "儲存", ja: "保存", ko: "저장", fr: "Enregistrer", de: "Speichern" }
    click_nothing { en: "Do nothing", zh: "不做任何事", tw: "不做任何事", ja: "何もしない", ko: "아무것도 안 함", fr: "Ne rien faire", de: "Nichts tun" }
    rec_max_seconds { en: "Longest recording", zh: "最长录制时长", tw: "最長錄製時長", ja: "最長録画時間", ko: "최대 녹화 시간", fr: "Durée maximale", de: "Maximale Aufnahmedauer" }
    rec_unlimited { en: "Unlimited", zh: "不限", tw: "不限", ja: "無制限", ko: "제한 없음", fr: "Illimité", de: "Unbegrenzt" }
    rec_mode { en: "Capture mode", zh: "录制模式", tw: "錄製模式", ja: "録画モード", ko: "녹화 모드", fr: "Mode de capture", de: "Aufnahmemodus" }
    rec_mode_region { en: "Region", zh: "区域", tw: "區域", ja: "範囲", ko: "영역", fr: "Zone", de: "Bereich" }
    rec_mode_screen { en: "Full screen", zh: "全屏", tw: "全螢幕", ja: "全画面", ko: "전체 화면", fr: "Plein écran", de: "Ganzer Bildschirm" }
    rec_mode_window { en: "Window", zh: "窗口", tw: "視窗", ja: "ウィンドウ", ko: "창", fr: "Fenêtre", de: "Fenster" }
    rec_resolution { en: "Resolution", zh: "分辨率", tw: "解析度", ja: "解像度", ko: "해상도", fr: "Résolution", de: "Auflösung" }
    rec_res_source { en: "Source", zh: "原始", tw: "原始", ja: "元サイズ", ko: "원본", fr: "Source", de: "Quelle" }
    rec_res_1080 { en: "1080p", zh: "1080p", tw: "1080p", ja: "1080p", ko: "1080p", fr: "1080p", de: "1080p" }
    rec_res_720 { en: "720p", zh: "720p", tw: "720p", ja: "720p", ko: "720p", fr: "720p", de: "720p" }
    rec_webcam { en: "Webcam (picture-in-picture)", zh: "摄像头画中画", tw: "攝影機子母畫面", ja: "ウェブカメラ (子画面)", ko: "웹캠 (화면 속 화면)", fr: "Webcam (incrustation)", de: "Webcam (Bild-in-Bild)" }
    rec_webcam_device { en: "Webcam device", zh: "摄像头设备", tw: "攝影機裝置", ja: "ウェブカメラデバイス", ko: "웹캠 장치", fr: "Périphérique webcam", de: "Webcam-Gerät" }
    rec_system_audio { en: "System audio", zh: "系统声音", tw: "系統聲音", ja: "システム音声", ko: "시스템 오디오", fr: "Audio système", de: "Systemaudio" }
    rec_system_audio_device { en: "System audio device", zh: "系统声音设备", tw: "系統聲音裝置", ja: "システム音声デバイス", ko: "시스템 오디오 장치", fr: "Périphérique audio système", de: "Systemaudio-Gerät" }
    rec_mic { en: "Microphone", zh: "麦克风", tw: "麥克風", ja: "マイク", ko: "마이크", fr: "Microphone", de: "Mikrofon" }
    rec_mic_device { en: "Microphone device", zh: "麦克风设备", tw: "麥克風裝置", ja: "マイクデバイス", ko: "마이크 장치", fr: "Périphérique micro", de: "Mikrofon-Gerät" }
    rec_device_hint { en: "Needs ffmpeg. Blank = default device (auto-detected).", zh: "需要 ffmpeg。留空用默认设备（自动检测）。", tw: "需要 ffmpeg。留空用預設裝置（自動偵測）。", ja: "ffmpeg が必要。空欄は既定デバイス（自動検出）。", ko: "ffmpeg 필요. 비우면 기본 장치(자동 감지).", fr: "Nécessite ffmpeg. Vide = périphérique par défaut (auto).", de: "Benötigt ffmpeg. Leer = Standardgerät (automatisch)." }
    rec_hud_recording { en: "Recording", zh: "录制中", tw: "錄製中", ja: "録画中", ko: "녹화 중", fr: "Enregistrement", de: "Aufnahme" }
    rec_countdown_label { en: "Countdown", zh: "录制倒计时", tw: "錄製倒數", ja: "録画カウントダウン", ko: "녹화 카운트다운", fr: "Compte à rebours", de: "Countdown" }
    rec_no_window { en: "No window found to record", zh: "没有找到可录制的窗口", tw: "沒有找到可錄製的視窗", ja: "録画するウィンドウが見つかりません", ko: "녹화할 창을 찾을 수 없습니다", fr: "Aucune fenêtre à enregistrer", de: "Kein aufzunehmendes Fenster gefunden" }
    rec_countdown_off { en: "Off", zh: "关", tw: "關", ja: "オフ", ko: "끄기", fr: "Désactivé", de: "Aus" }
    rec_hud_paused { en: "Paused", zh: "已暂停", tw: "已暫停", ja: "一時停止", ko: "일시 정지", fr: "En pause", de: "Pausiert" }
    rec_hud_pause { en: "Pause", zh: "暂停", tw: "暫停", ja: "一時停止", ko: "일시 정지", fr: "Pause", de: "Pause" }
    rec_hud_resume { en: "Resume", zh: "继续", tw: "繼續", ja: "再開", ko: "재개", fr: "Reprendre", de: "Fortsetzen" }
    rec_hud_stop { en: "Stop", zh: "停止", tw: "停止", ja: "停止", ko: "정지", fr: "Arrêter", de: "Stopp" }
    rec_hud_hint { en: "Esc stops the recording", zh: "按 Esc 停止录制", tw: "按 Esc 停止錄製", ja: "Esc で録画を停止", ko: "Esc로 녹화 정지", fr: "Échap arrête l'enregistrement", de: "Esc beendet die Aufnahme" }
    rec_annotate { en: "Annotate", zh: "标注", tw: "標註", ja: "注釈", ko: "주석", fr: "Annoter", de: "Anmerken" }
    rec_truncated { en: "Recording interrupted, saved truncated clip", zh: "录制被中断，已保存被截断的视频", tw: "錄製被中斷，已儲存被截斷的影片", ja: "録画が中断されました。切り詰められた動画を保存しました", ko: "녹화가 중단되어 잘린 영상을 저장했습니다", fr: "Enregistrement interrompu, vidéo tronquée enregistrée", de: "Aufnahme unterbrochen, verkürztes Video gespeichert" }
    mk_arrow { en: "Arrow", zh: "箭头", tw: "箭頭", ja: "矢印", ko: "화살표", fr: "Flèche", de: "Pfeil" }
    mk_line { en: "Line", zh: "直线", tw: "直線", ja: "直線", ko: "직선", fr: "Ligne", de: "Linie" }
    mk_rect { en: "Rectangle", zh: "矩形", tw: "矩形", ja: "矩形", ko: "사각형", fr: "Rectangle", de: "Rechteck" }
    mk_ellipse { en: "Ellipse", zh: "椭圆", tw: "橢圓", ja: "楕円", ko: "타원", fr: "Ellipse", de: "Ellipse" }
    mk_text { en: "Text", zh: "文字", tw: "文字", ja: "テキスト", ko: "텍스트", fr: "Texte", de: "Text" }
    mk_emoji { en: "Emoji", zh: "表情", tw: "表情", ja: "絵文字", ko: "이모지", fr: "Emoji", de: "Emoji" }
    mk_clear { en: "Clear", zh: "清除", tw: "清除", ja: "クリア", ko: "지우기", fr: "Effacer", de: "Löschen" }
    mk_close { en: "Close", zh: "关闭", tw: "關閉", ja: "閉じる", ko: "닫기", fr: "Fermer", de: "Schließen" }
    mk_title { en: "Recording annotation", zh: "录制标注", tw: "錄製標註", ja: "録画への注釈", ko: "녹화 주석", fr: "Annotation de l'enregistrement", de: "Aufnahme-Anmerkung" }
    mk_width { en: "Width", zh: "粗细", tw: "粗細", ja: "太さ", ko: "굵기", fr: "Épaisseur", de: "Stärke" }
    mk_size { en: "Size", zh: "字号", tw: "字號", ja: "サイズ", ko: "크기", fr: "Taille", de: "Größe" }
    mk_passthrough { en: "Click-through", zh: "点击穿透", tw: "點擊穿透", ja: "クリック透過", ko: "클릭 통과", fr: "Clic traversant", de: "Klick-Durchlass" }
    mk_passthrough_hint { en: "While on, clicks reach the window below. Pick a tool to draw on the recording.", zh: "开启时点击会传到下层窗口。选择工具即可在画面上绘制。", tw: "開啟時點擊會傳到下層視窗。選擇工具即可在畫面上繪製。", ja: "オンのあいだクリックは下のウィンドウに届きます。ツールを選ぶと描画できます。", ko: "켜져 있는 동안 클릭은 아래 창으로 전달됩니다. 도구를 선택하면 그릴 수 있습니다.", fr: "Activé, les clics atteignent la fenêtre du dessous. Choisissez un outil pour dessiner.", de: "Ein: Klicks erreichen das Fenster darunter. Werkzeug wählen, um zu zeichnen." }
    mk_text_hint { en: "Type, then press Enter", zh: "输入文字后按 Enter", tw: "輸入文字後按 Enter", ja: "入力して Enter を押す", ko: "입력 후 Enter 를 누르세요", fr: "Saisissez, puis Entrée", de: "Text eingeben, dann Enter" }
    mk_emoji_title { en: "Pick an emoji", zh: "选择表情", tw: "選擇表情", ja: "絵文字を選択", ko: "이모지 선택", fr: "Choisir un emoji", de: "Emoji wählen" }
    mk_cleared { en: "Annotations cleared", zh: "已清除标注", tw: "已清除標註", ja: "注釈をクリアしました", ko: "주석을 지웠습니다", fr: "Annotations effacées", de: "Anmerkungen gelöscht" }
    cap_sound { en: "Sound", zh: "音效", tw: "音效", ja: "効果音", ko: "효과음", fr: "Son", de: "Ton" }
    cap_sound_copy { en: "When copied to the clipboard", zh: "复制到剪贴板时", tw: "複製到剪貼簿時", ja: "コピー時", ko: "복사할 때", fr: "Lors d'une copie", de: "Beim Kopieren" }
    cap_sound_save { en: "When saved to a file", zh: "保存到文件时", tw: "儲存到檔案時", ja: "保存時", ko: "저장할 때", fr: "Lors d'un enregistrement", de: "Beim Speichern" }
    cap_sound_file { en: "Notification sound", zh: "提示音文件", tw: "提示音檔案", ja: "通知音ファイル", ko: "알림음 파일", fr: "Son de notification", de: "Benachrichtigungston" }
    cap_sound_browse { en: "Browse…", zh: "浏览…", tw: "瀏覽…", ja: "参照…", ko: "찾아보기…", fr: "Parcourir…", de: "Durchsuchen…" }
    cap_magnifier { en: "Magnifier", zh: "放大镜", tw: "放大鏡", ja: "拡大鏡", ko: "돋보기", fr: "Loupe", de: "Lupe" }
    cap_magnifier_size { en: "Magnifier area", zh: "放大镜范围", tw: "放大鏡範圍", ja: "拡大範囲", ko: "돋보기 범위", fr: "Zone de la loupe", de: "Lupenbereich" }
    cap_color_format { en: "Colour value", zh: "颜色值格式", tw: "顏色值格式", ja: "色の表記", ko: "색상 표기", fr: "Format de couleur", de: "Farbformat" }
    cap_pointer { en: "Capture the mouse cursor", zh: "捕捉鼠标指针", tw: "捕捉滑鼠指標", ja: "マウスカーソルを含める", ko: "마우스 커서 포함", fr: "Inclure le curseur", de: "Mauszeiger aufnehmen" }
    fmt_hex { en: "HEX", zh: "十六进制", tw: "十六進位", ja: "HEX", ko: "HEX", fr: "HEX", de: "HEX" }
    fmt_rgb { en: "RGB", zh: "RGB", tw: "RGB", ja: "RGB", ko: "RGB", fr: "RVB", de: "RGB" }
    fmt_hsl { en: "HSL", zh: "HSL", tw: "HSL", ja: "HSL", ko: "HSL", fr: "TSL", de: "HSL" }
    long_settings { en: "Scrolling capture", zh: "长截图", tw: "長截圖", ja: "スクロール撮影", ko: "스크롤 캡처", fr: "Capture défilante", de: "Scroll-Aufnahme" }
    long_max_height { en: "Maximum height", zh: "最长结果高度", tw: "最長結果高度", ja: "結果の最大高さ", ko: "최대 결과 높이", fr: "Hauteur maximale", de: "Maximale Höhe" }
    long_scroll_ticks { en: "Scroll per step", zh: "每次滚动行数", tw: "每次滾動行數", ja: "1ステップのスクロール量", ko: "단계당 스크롤 량", fr: "Défilement par pas", de: "Scroll pro Schritt" }
    long_scroll_delay { en: "Settle time", zh: "滚动后等待", tw: "滾動後等待", ja: "スクロール後の待機", ko: "스크롤 후 대기", fr: "Temps de stabilisation", de: "Wartezeit" }
    long_diff_threshold { en: "Motion threshold", zh: "静止判定阈值", tw: "靜止判定閾值", ja: "動き判定しきい値", ko: "움직임 판단 임계값", fr: "Seuil de mouvement", de: "Bewegungsschwelle" }
    long_idle_limit { en: "Still rounds (auto)", zh: "静止轮数（自动）", tw: "靜止輪數（自動）", ja: "静止ラウンド数（自動）", ko: "정지 라운드 수(자동)", fr: "Tours immobiles (auto)", de: "Stille Runden (auto)" }
    long_manual_idle { en: "Still rounds (manual)", zh: "静止轮数（手动）", tw: "靜止輪數（手動）", ja: "静止ラウンド数（手動）", ko: "정지 라운드 수(수동)", fr: "Tours immobiles (manuel)", de: "Stille Runden (manuell)" }
    long_max_frames { en: "Sampling rounds", zh: "采样轮数", tw: "取樣輪數", ja: "サンプリング回数", ko: "샘플링 라운드", fr: "Tours d'échantillonnage", de: "Sampling-Runden" }
    long_settings_hint { en: "A higher threshold ends the capture sooner. In manual mode you roll the wheel yourself and Esc finishes.", zh: "阈值越大越早结束。手动模式下由你滚动滚轮，按 Esc 完成。", tw: "閾值越大越早結束。手動模式下由你滾動滾輪，按 Esc 完成。", ja: "しきい値が大きいほど早く終了します。手動モードでは自分でホイールを回し、Esc で完了します。", ko: "임계값이 클수록 더 일찍 종료됩니다. 수동 모드에서는 직접 휠을 굴리고 Esc 로 완료합니다.", fr: "Un seuil plus élevé termine plus tôt. En mode manuel, vous tournez la molette et Échap termine.", de: "Ein höherer Schwellwert beendet früher. Im manuellen Modus scrollen Sie selbst, Esc beendet." }
    long_hud_hint { en: "Esc, a mouse click or the button ends the capture", zh: "按 Esc、点击鼠标或按按钮结束", tw: "按 Esc、點擊滑鼠或按按鈕結束", ja: "Esc、マウスクリック、またはボタンで終了", ko: "Esc, 마우스 클릭 또는 버튼으로 종료", fr: "Échap, un clic ou le bouton termine la capture", de: "Esc, ein Klick oder die Schaltfläche beendet" }
    hdr_state { en: "HDR display", zh: "HDR 显示器", tw: "HDR 顯示器", ja: "HDR ディスプレイ", ko: "HDR 디스플레이", fr: "Écran HDR", de: "HDR-Display" }
    hdr_detected { en: "HDR active", zh: "已开启 HDR", tw: "已開啟 HDR", ja: "HDR 有効", ko: "HDR 켜짐", fr: "HDR actif", de: "HDR aktiv" }
    hdr_none { en: "SDR", zh: "未检测到", tw: "未偵測到", ja: "未検出", ko: "감지되지 않음", fr: "SDR", de: "SDR" }
    hdr_fix { en: "Fix washed-out captures", zh: "修正 HDR 发白", tw: "修正 HDR 發白", ja: "HDR の白飛びを補正", ko: "HDR 밀림 보정", fr: "Corriger les captures délavées", de: "Blasse Aufnahmen korrigieren" }
    hdr_fix_hint { en: "Displays in HDR mode return washed-out frames through the capture pipeline. With this on, captures go through the SDR path instead.", zh: "显示器处于 HDR 模式时，捕获的画面会发白。开启后截图改走 SDR 捕获通道。", tw: "顯示器處於 HDR 模式時，捕獲的畫面會發白。開啟後截圖改走 SDR 捕獲通道。", ja: "HDR モードのディスプレイでは、取り込んだ画面が白飛びします。有効にすると SDR 経路で取り込みます。", ko: "HDR 모드 디스플레에서는 캡처가 밀려 보입니다. 활성화하면 SDR 경로로 캡처합니다.", fr: "Les écrans en HDR donnent des captures délavées. Activé, les captures passent par le chemin SDR.", de: "Displays im HDR-Modus liefern blasse Aufnahmen. Damit aktiv, gehen Aufnahmen über den SDR-Pfad." }
    hdr_toast { en: "HDR display detected — captures may look washed out", zh: "检测到 HDR 显示器，截图可能发白", tw: "偵測到 HDR 顯示器，截圖可能發白", ja: "HDR ディスプレイを検出 — スクリーンショットが白飛びする場合があります", ko: "HDR 디스플레이 감지 — 캡처가 밀려 보일 수 있습니다", fr: "Écran HDR détecté — les captures peuvent être délavées", de: "HDR-Display erkannt — Aufnahmen können blass wirken" }
    pin_restore_limit { en: "Restorable pins", zh: "可恢复贴图数", tw: "可復原貼圖數", ja: "復元できるピン数", ko: "복원 가능한 핀 수", fr: "Épingles restaurables", de: "Wiederherstellbare Pins" }
    tray_icon_style { en: "Tray icon", zh: "托盘图标样式", tw: "托盤圖示樣式", ja: "トレイアイコン", ko: "트레이 아이콘", fr: "Icône de la zone de notification", de: "Tray-Symbol" }
    tray_icon_auto { en: "Automatic", zh: "自动", tw: "自動", ja: "自動", ko: "자동", fr: "Automatique", de: "Automatisch" }
    tray_icon_white { en: "White", zh: "白色", tw: "白色", ja: "白", ko: "흰색", fr: "Blanc", de: "Weiß" }
    tray_icon_color { en: "Colour", zh: "彩色", tw: "彩色", ja: "カラー", ko: "컬러", fr: "Couleur", de: "Farbig" }
    tray_icon_custom { en: "Custom image", zh: "自定义图片", tw: "自訂圖片", ja: "カスタム画像", ko: "사용자 이미지", fr: "Image personnalisée", de: "Eigenes Bild" }
    gen_admin_restart { en: "Restart as administrator", zh: "以管理员身份重启", tw: "以系統管理員身分重啟", ja: "管理者として再起動", ko: "관리자 권한으로 재시작", fr: "Redémarrer en administrateur", de: "Als Administrator neu starten" }
    gen_admin_failed { en: "Could not restart with elevation", zh: "无法以管理员身份重启", tw: "無法以系統管理員身分重啟", ja: "管理者として再起動できません", ko: "관리자 권한으로 재시작할 수 없음", fr: "Échec du redémarrage élevé", de: "Neustart mit Rechten fehlgeschlagen" }
    pin_group_default { en: "Default group", zh: "默认分组", tw: "預設分組", ja: "既定のグループ", ko: "기본 그룹", fr: "Groupe par défaut", de: "Standardgruppe" }
    pin_group_last { en: "At least one group must remain", zh: "至少需要保留一个分组", tw: "至少需要保留一個分組", ja: "グループを 1 つ以上残してください", ko: "하나 이상의 그룹이 필요합니다", fr: "Il faut garder au moins un groupe", de: "Mindestens eine Gruppe muss bleiben" }
    pin_group_new { en: "New group", zh: "新建分组", tw: "新增分組", ja: "新しいグループ", ko: "새 그룹", fr: "Nouveau groupe", de: "Neue Gruppe" }
    pin_group_rename { en: "Rename", zh: "重命名", tw: "重新命名", ja: "名前を変更", ko: "이름 바꾸기", fr: "Renommer", de: "Umbenennen" }
    pin_group_delete { en: "Delete group", zh: "删除分组", tw: "刪除分組", ja: "グループを削除", ko: "그룹 삭제", fr: "Supprimer le groupe", de: "Gruppe löschen" }
    pin_group_hide { en: "Hide the group", zh: "隐藏整组", tw: "隱藏整組", ja: "グループを隠す", ko: "그룹 숨기기", fr: "Masquer le groupe", de: "Gruppe ausblenden" }
    pin_group_show { en: "Show the group", zh: "显示整组", tw: "顯示整組", ja: "グループを表示", ko: "그룹 표시", fr: "Afficher le groupe", de: "Gruppe einblenden" }
    pin_group_active { en: "New pins join this group", zh: "新贴图加入此分组", tw: "新貼圖加入此分組", ja: "新しいピンはこのグループに追加", ko: "새 핀이 이 그룹에 추가됩니다", fr: "Les nouvelles épingles rejoignent ce groupe", de: "Neue Pins kommen in diese Gruppe" }
    pin_group_move { en: "Move to group", zh: "移动到分组", tw: "移動到分組", ja: "グループへ移動", ko: "그룹으로 이동", fr: "Déplacer vers le groupe", de: "In Gruppe verschieben" }
    pin_group_hint { en: "Group pins so a whole set can be hidden or closed at once.", zh: "把贴图编组，即可整组隐藏或关闭。", tw: "把貼圖編組，即可整組隱藏或關閉。", ja: "ピンをグループ化すると、まとめて隠したり閉じたりできます。", ko: "핀을 그룹화하면 한 번에 숨기거나 닫을 수 있습니다.", fr: "Groupez les épingles pour toutes les masquer ou fermer d'un coup.", de: "Gruppieren Sie Pins, um sie gemeinsam auszublenden oder zu schließen." }
    pin_group_name_hint { en: "Group name", zh: "分组名称", tw: "分组名稱", ja: "グループ名", ko: "그룹 이름", fr: "Nom du groupe", de: "Gruppenname" }
    pin_group_created { en: "Group created", zh: "已新建分组", tw: "已新增分組", ja: "グループを作成しました", ko: "그룹을 만들었습니다", fr: "Groupe créé", de: "Gruppe erstellt" }
    pin_group_deleted { en: "Group deleted; its pins moved to the first group", zh: "分组已删除，其中的贴图已移到第一个分组", tw: "分組已刪除，其中的貼圖已移到第一個分組", ja: "グループを削除し、ピンを最初のグループへ移動しました", ko: "그룹을 삭제하고 핀을 첫 그룹으로 이동했습니다", fr: "Groupe supprimé ; ses épingles ont été déplacées", de: "Gruppe gelöscht; seine Pins wurden verschoben" }
    pin_no_text { en: "This pin holds an image, not text", zh: "该贴图是图片，没有文字可复制", tw: "該貼圖是圖片，沒有文字可複製", ja: "このピンは画像で、テキストはありません", ko: "이 핀은 이미지라 복사할 텍스트가 없습니다", fr: "Cette épingle est une image, pas du texte", de: "Dieser Pin ist ein Bild, kein Text" }
    gestures_new_pins_only { en: "Saved. Pins that are already open keep their current bindings.", zh: "已保存。已经打开的贴图仍沿用当前绑定。", tw: "已儲存。已經開啟的貼圖仍沿用目前綁定。", ja: "保存しました。すでに開いているピンは現在の設定のままです。", ko: "저장됨. 이미 열린 핀은 현재 설정을 유지합니다.", fr: "Enregistré. Les épingles déjà ouvertes gardent leurs réglages.", de: "Gespeichert. Bereits geöffnete Pins behalten ihre Einstellungen." }
    ctl_gestures { en: "Pin gestures", zh: "贴图手势", tw: "貼圖手勢", ja: "ピンのジェスチャー", ko: "핀 제스처", fr: "Gestes des épingles", de: "Pin-Gesten" }
    ctl_gestures_hint { en: "Pick a modifier and a button for each action. Conflicts resolve top-down.", zh: "为每个操作选择修饰键和鼠标键。如有冲突，从上到下优先匹配。", tw: "為每個操作選擇修飾鍵和滑鼠鍵。如有衝突，由上到下優先匹配。", ja: "操作ごとに修飾キーとボタンを選びます。衝突時は上から順に適用されます。", ko: "각 작업별로 수정 키와 버튼을 선택하세요. 충돌 시 위에서 아래로 적용됩니다.", fr: "Choisissez un modificateur et un bouton pour chaque action. Les conflits se résolvent de haut en bas.", de: "Wählen Sie Modifikator und Taste pro Aktion. Konflikte werden von oben nach unten aufgelöst." }
    gst_zoom { en: "Zoom the pin", zh: "贴图缩放", tw: "貼圖縮放", ja: "ピンの拡大", ko: "핀 확대", fr: "Zoomer l'épingle", de: "Pin zoomen" }
    gst_opacity { en: "Adjust opacity", zh: "调整不透明度", tw: "調整不透明度", ja: "不透明度を変更", ko: "불투명도 조절", fr: "Régler l'opacité", de: "Deckkraft ändern" }
    gst_close { en: "Close the pin", zh: "关闭贴图", tw: "關閉貼圖", ja: "ピンを閉じる", ko: "핀 닫기", fr: "Fermer l'épingle", de: "Pin schließen" }
    gst_reset { en: "Reset the pin", zh: "重置贴图", tw: "重設貼圖", ja: "ピンをリセット", ko: "핀 초기화", fr: "Réinitialiser l'épingle", de: "Pin zurücksetzen" }
    gst_thumbnail { en: "Shrink to a thumbnail", zh: "缩略图", tw: "縮圖", ja: "サムネイルに縮小", ko: "섬네일로 축소", fr: "Réduire en vignette", de: "Als Miniatur verkleinern" }
    gst_copy_text { en: "Copy the pin's text", zh: "复制文本", tw: "複製文字", ja: "ピンの文字をコピー", ko: "핀 텍스트 복사", fr: "Copier le texte", de: "Text kopieren" }
    gst_none { en: "None", zh: "无", tw: "無", ja: "なし", ko: "없음", fr: "Aucun", de: "Keine" }
    gst_double { en: "Double-click", zh: "双击", tw: "雙擊", ja: "ダブルクリック", ko: "더블 클릭", fr: "Double-clic", de: "Doppelklick" }
    gst_wheel { en: "Wheel", zh: "滚轮", tw: "滾輪", ja: "ホイール", ko: "휠", fr: "Molette", de: "Rad" }
    gst_left { en: "Left", zh: "左键", tw: "左鍵", ja: "左ボタン", ko: "왼쪽", fr: "Gauche", de: "Links" }
    gst_middle { en: "Middle", zh: "中键", tw: "中鍵", ja: "中ボタン", ko: "가운데", fr: "Milieu", de: "Mitte" }
    gst_right { en: "Right", zh: "右键", tw: "右鍵", ja: "右ボタン", ko: "오른쪽", fr: "Droite", de: "Rechts" }
    gst_ctrl { en: "Ctrl", zh: "Ctrl", tw: "Ctrl", ja: "Ctrl", ko: "Ctrl", fr: "Ctrl", de: "Strg" }
    gst_shift { en: "Shift", zh: "Shift", tw: "Shift", ja: "Shift", ko: "Shift", fr: "Maj", de: "Umschalt" }
    gst_alt { en: "Alt", zh: "Alt", tw: "Alt", ja: "Alt", ko: "Alt", fr: "Alt", de: "Alt" }
    pin_restore_action { en: "Restore the last pin", zh: "恢复上一个贴图", tw: "復原上一個貼圖", ja: "最後のピンを復元", ko: "마지막 핀 복원", fr: "Restaurer la dernière épingle", de: "Letzten Pin wiederherstellen" }
    pin_restored { en: "Pin restored", zh: "已恢复贴图", tw: "已復原貼圖", ja: "ピンを復元しました", ko: "핀을 복원했습니다", fr: "Épingle restaurée", de: "Pin wiederhergestellt" }
    pin_nothing_to_restore { en: "No pin to restore", zh: "没有可恢复的贴图", tw: "沒有可復原的貼圖", ja: "復元できるピンがありません", ko: "복원할 핀이 없습니다", fr: "Aucune épingle à restaurer", de: "Kein Pin zum Wiederherstellen" }
    pin_shadow_color { en: "Shadow colour", zh: "阴影颜色", tw: "陰影顏色", ja: "影の色", ko: "그림자 색", fr: "Couleur de l'ombre", de: "Schattenfarbe" }
    pin_zoom_at_cursor { en: "Zoom around the cursor", zh: "滚轮缩放以指针为中心", tw: "滾輪縮放以指標為中心", ja: "カーソル中心に拡大", ko: "커서 중심 확대", fr: "Zoom autour du curseur", de: "Um den Cursor zoomen" }
    pin_smooth_zoom { en: "Smooth zoom", zh: "平滑缩放", tw: "平滑縮放", ja: "スムーズズーム", ko: "부드러운 확대", fr: "Zoom fluide", de: "Sanftes Zoomen" }
    pin_text_font_size { en: "Font size", zh: "字体大小", tw: "字型大小", ja: "文字サイズ", ko: "글꼴 크기", fr: "Taille de police", de: "Schriftgröße" }
    pin_text_bg { en: "Background", zh: "背景颜色", tw: "背景顏色", ja: "背景色", ko: "배경색", fr: "Arrière-plan", de: "Hintergrund" }
    pin_text_fg { en: "Text colour", zh: "文本颜色", tw: "文字顏色", ja: "文字色", ko: "글자 색", fr: "Couleur du texte", de: "Textfarbe" }
    pin_skip_file_paths { en: "Do not turn a file path into a pin", zh: "不把文件路径转化成贴图", tw: "不把檔案路徑轉成貼圖", ja: "ファイルパスをピンにしない", ko: "파일 경로를 핀으로 만들지 않음", fr: "Ne pas transformer un chemin de fichier", de: "Dateipfad nicht anheften" }
    gen_auto_backup { en: "Back up the config on save", zh: "保存时自动备份配置", tw: "儲存時自動備份設定", ja: "保存時に設定をバックアップ", ko: "저장 시 설정 백업", fr: "Sauvegarder la config", de: "Konfiguration sichern" }
    gen_enhanced_tray { en: "Enhanced tray menu", zh: "增强版托盘菜单", tw: "增強版托盤選單", ja: "拡張トレイメニュー", ko: "확장 트레이 메뉴", fr: "Menu de la zone de notification enrichi", de: "Erweitertes Tray-Menü" }
    gen_log_level { en: "Log level", zh: "日志级别", tw: "日誌等級", ja: "ログレベル", ko: "로그 수준", fr: "Niveau de journal", de: "Protokollstufe" }
    log_error { en: "Error", zh: "错误", tw: "錯誤", ja: "エラー", ko: "오류", fr: "Erreur", de: "Fehler" }
    log_warn { en: "Warning", zh: "警告", tw: "警告", ja: "警告", ko: "경고", fr: "Avertissement", de: "Warnung" }
    log_info { en: "Info", zh: "信息", tw: "資訊", ja: "情報", ko: "정보", fr: "Info", de: "Info" }
    log_debug { en: "Debug", zh: "调试", tw: "偵錯", ja: "デバッグ", ko: "디버그", fr: "Débogage", de: "Debug" }
    log_trace { en: "Trace", zh: "跟踪", tw: "追蹤", ja: "トレース", ko: "추적", fr: "Trace", de: "Trace" }
    pin_shadow { en: "Window shadow", zh: "窗口阴影", tw: "視窗陰影", ja: "ウィンドウの影", ko: "창 그림자", fr: "Ombre de fenêtre", de: "Fensterschatten" }
    pin_opacity { en: "Default opacity", zh: "默认不透明度", tw: "預設不透明度", ja: "既定の不透明度", ko: "기본 불투명도", fr: "Opacité par défaut", de: "Standarddeckkraft" }
    pin_max_size { en: "Max window size", zh: "最大窗口尺寸", tw: "最大視窗尺寸", ja: "最大ウィンドウサイズ", ko: "최대 창 크기", fr: "Taille max.", de: "Max. Fenstergröße" }
    pin_thumb { en: "Thumbnail size", zh: "缩略图尺寸", tw: "縮圖尺寸", ja: "サムネイルサイズ", ko: "썸네일 크기", fr: "Taille des vignettes", de: "Vorschaugröße" }
    ctl_app_keys { en: "In-app shortcuts", zh: "应用内快捷键", tw: "應用內快捷鍵", ja: "アプリ内ショートカット", ko: "앱 내 단축키", fr: "Raccourcis internes", de: "Interne Kürzel" }
    ctl_app_keys_hint { en: "Used inside the capture overlay. Click a field and press the keys.", zh: "在截图覆盖层内生效。点输入框后直接按组合键。", tw: "在截圖覆蓋層內生效。點輸入框後直接按組合鍵。", ja: "キャプチャ画面内で有効。欄をクリックしてキーを押します。", ko: "캡처 오버레이에서 사용됩니다. 입력란을 클릭하고 키를 누르세요.", fr: "Utilisés dans la capture. Cliquez puis appuyez sur les touches.", de: "Gelten im Aufnahme-Overlay. Feld anklicken, dann Tasten drücken." }
    key_confirm { en: "Confirm", zh: "确认（复制）", tw: "確認（複製）", ja: "確定（コピー）", ko: "확인(복사)", fr: "Valider", de: "Bestätigen" }
    key_pin { en: "Pin", zh: "贴图", tw: "貼圖", ja: "ピン", ko: "고정", fr: "Épingler", de: "Anheften" }
    key_copy { en: "Copy", zh: "复制", tw: "複製", ja: "コピー", ko: "복사", fr: "Copier", de: "Kopieren" }
    key_save { en: "Save", zh: "保存", tw: "儲存", ja: "保存", ko: "저장", fr: "Enregistrer", de: "Speichern" }
    key_cancel { en: "Cancel", zh: "取消", tw: "取消", ja: "中止", ko: "취소", fr: "Annuler", de: "Abbrechen" }
    key_undo { en: "Undo", zh: "撤销", tw: "還原", ja: "元に戻す", ko: "실행 취소", fr: "Annuler", de: "Rückgängig" }
    key_redo { en: "Redo", zh: "重做", tw: "重做", ja: "やり直す", ko: "다시 실행", fr: "Rétablir", de: "Wiederholen" }
    ctl_dup { en: "Used twice", zh: "重复", tw: "重複", ja: "重複", ko: "중복", fr: "En double", de: "Doppelt" }
    ctl_sub_global { en: "Global hotkeys", zh: "全局快捷键", tw: "全域快速键", ja: "グローバルショートカット", ko: "전역 단축키", fr: "Raccourcis globaux", de: "Globale Kürzel" }
    ctl_sub_app { en: "In-app shortcuts", zh: "应用内快捷键", tw: "應用內快速鍵", ja: "アプリ内ショートカット", ko: "앱 내 단축키", fr: "Raccourcis internes", de: "Interne Kürzel" }
    ctl_sub_mouse { en: "Mouse", zh: "鼠标", tw: "滑鼠", ja: "マウス", ko: "마우스", fr: "Souris", de: "Maus" }
    ctl_mouse { en: "Mouse", zh: "鼠标", tw: "滑鼠", ja: "マウス", ko: "마우스", fr: "Souris", de: "Maus" }
    ctl_mouse_hint { en: "How the mouse behaves on pinned images.", zh: "鼠标在贴图上的行为。", tw: "滑鼠在貼圖上的行為。", ja: "ピン上のマウスの動作。", ko: "핀 위의 마우스 동작.", fr: "Comportement de la souris sur les épingles.", de: "Mausverhalten auf angehefteten Bildern." }
    ctl_mouse_through { en: "New pins start click-through", zh: "新贴图默认鼠标穿透", tw: "新貼圖預設滑鼠穿透", ja: "新しいピンをクリック通過で開始", ko: "새 핀을 클릭 통과로 시작", fr: "Nouvelles épingles cliquables-au-travers", de: "Neue Pins klick-durch" }
    upd_on_start { en: "Check on startup", zh: "启动时检查更新", tw: "啟動時檢查更新", ja: "起動時に確認", ko: "시작 시 확인", fr: "Vérifier au démarrage", de: "Beim Start prüfen" }
    upd_hours { en: "Interval (hours)", zh: "检查间隔（小时）", tw: "檢查間隔（小時）", ja: "間隔（時間）", ko: "간격(시간)", fr: "Intervalle (heures)", de: "Intervall (Stunden)" }
    upd_beta { en: "Include beta builds", zh: "包含 beta 版本", tw: "包含 beta 版本", ja: "ベータ版を含める", ko: "베타 포함", fr: "Inclure les bêtas", de: "Beta einbeziehen" }
    upd_check_now { en: "Check now", zh: "立即检查", tw: "立即檢查", ja: "今すぐ確認", ko: "지금 확인", fr: "Vérifier", de: "Jetzt prüfen" }
    upd_note { en: "Checks GitHub Releases and downloads the build that matches this computer.", zh: "从 GitHub Releases 检查更新，并下载适配本机的版本。", tw: "從 GitHub Releases 檢查更新，並下載符合本機的版本。", ja: "GitHub Releases を確認し、この PC に合ったビルドをダウンロードします。", ko: "GitHub Releases를 확인하고 이 PC에 맞는 빌드를 다운로드합니다.", fr: "Vérifie les versions GitHub et télécharge le build adapté à cet ordinateur.", de: "Prüft GitHub-Releases und lädt den passenden Build für diesen Computer." }
    upd_idle { en: "Click check to look for a newer version.", zh: "点击「立即检查」查找新版本。", tw: "點擊「立即檢查」尋找新版本。", ja: "「今すぐ確認」で新しいバージョンを探します。", ko: "지금 확인을 눌러 새 버전을 찾으세요.", fr: "Cliquez sur Vérifier pour chercher une nouvelle version.", de: "Auf „Jetzt prüfen“ klicken, um nach einer neueren Version zu suchen." }
    upd_checking { en: "Checking for updates…", zh: "正在检查更新…", tw: "正在檢查更新…", ja: "更新を確認しています…", ko: "업데이트 확인 중…", fr: "Recherche de mises à jour…", de: "Suche nach Updates…" }
    upd_uptodate { en: "You are up to date", zh: "已是最新版本", tw: "已是最新版本", ja: "最新です", ko: "최신 버전입니다", fr: "Vous êtes à jour", de: "Sie sind auf dem neuesten Stand" }
    upd_available { en: "New version available", zh: "发现新版本", tw: "發現新版本", ja: "新しいバージョンがあります", ko: "새 버전이 있습니다", fr: "Nouvelle version disponible", de: "Neue Version verfügbar" }
    upd_available_toast { en: "New version available", zh: "发现新版本", tw: "發現新版本", ja: "新しいバージョンがあります", ko: "새 버전이 있습니다", fr: "Nouvelle version disponible", de: "Neue Version verfügbar" }
    upd_download { en: "Download", zh: "下载", tw: "下載", ja: "ダウンロード", ko: "다운로드", fr: "Télécharger", de: "Herunterladen" }
    upd_cancel { en: "Cancel", zh: "取消", tw: "取消", ja: "キャンセル", ko: "취소", fr: "Annuler", de: "Abbrechen" }
    upd_downloaded { en: "Downloaded to", zh: "已下载到", tw: "已下載到", ja: "ダウンロード先", ko: "다운로드 위치", fr: "Téléchargé dans", de: "Heruntergeladen nach" }
    upd_open_folder { en: "Show in folder", zh: "打开所在文件夹", tw: "開啟所在資料夾", ja: "フォルダーを開く", ko: "폴더 열기", fr: "Afficher dans le dossier", de: "Im Ordner anzeigen" }
    upd_install { en: "Install and restart", zh: "安装并重启", tw: "安裝並重新啟動", ja: "インストールして再起動", ko: "설치 후 재시작", fr: "Installer et redémarrer", de: "Installieren und neu starten" }
    upd_open_page { en: "Releases page", zh: "打开 Releases 页面", tw: "開啟 Releases 頁面", ja: "Releases ページ", ko: "Releases 페이지", fr: "Page des versions", de: "Releases-Seite" }
    upd_no_asset { en: "This release has no build for your platform.", zh: "该版本没有适配本机的安装包。", tw: "該版本沒有符合本機的安裝包。", ja: "このリリースにはこの環境向けのビルドがありません。", ko: "이 릴리스에는 이 플랫폼용 빌드가 없습니다.", fr: "Cette version n'a pas de build pour votre plateforme.", de: "Diese Version hat keinen Build für Ihre Plattform." }
    upd_check_failed { en: "Update check failed", zh: "检查更新失败", tw: "檢查更新失敗", ja: "更新の確認に失敗しました", ko: "업데이트 확인 실패", fr: "Échec de la vérification", de: "Update-Prüfung fehlgeschlagen" }
    upd_download_failed { en: "Download failed", zh: "下载失败", tw: "下載失敗", ja: "ダウンロードに失敗しました", ko: "다운로드 실패", fr: "Échec du téléchargement", de: "Download fehlgeschlagen" }
    upd_changelog { en: "Changelog", zh: "更新日志", tw: "更新日誌", ja: "更新履歴", ko: "변경 내역", fr: "Journal des modifications", de: "Änderungsprotokoll" }
    upd_changelog_hint { en: "What changed in each version.", zh: "查看各版本的更新内容。", tw: "查看各版本的更新內容。", ja: "各バージョンの変更点。", ko: "각 버전의 변경 내용을 확인하세요.", fr: "Les changements de chaque version.", de: "Was sich in jeder Version geändert hat." }
    upd_github_release { en: "GitHub Release", zh: "GitHub Release", tw: "GitHub Release", ja: "GitHub Release", ko: "GitHub Release", fr: "GitHub Release", de: "GitHub Release" }
    upd_no_notes { en: "No release notes.", zh: "该版本没有更新说明。", tw: "該版本沒有更新說明。", ja: "このバージョンの更新内容はありません。", ko: "이 버전에는 변경 내역이 없습니다.", fr: "Aucune note de version.", de: "Keine Versionshinweise." }
    cfg_dir_change { en: "Change…", zh: "更改…", tw: "更改…", ja: "変更…", ko: "변경…", fr: "Changer…", de: "Ändern…" }
    cfg_move_title { en: "Change config folder", zh: "更改配置文件夹", tw: "更改設定資料夾", ja: "設定フォルダーを変更", ko: "설정 폴더 변경", fr: "Changer le dossier de configuration", de: "Konfigurationsordner ändern" }
    cfg_move_body { en: "Copy the current configuration (config.json, background images, …) to the new folder?\n\nNew folder:\n{dir}\n\nPinSight will use this folder from the next launch on.", zh: "是否把当前配置（config.json、背景图等）复制到新文件夹？\n\n新文件夹：\n{dir}\n\n下次启动起 PinSight 将使用该文件夹。", tw: "是否把目前設定（config.json、背景圖等）複製到新資料夾？\n\n新資料夾：\n{dir}\n\n下次啟動起 PinSight 將使用該資料夾。", ja: "現在の設定（config.json、背景画像など）を新しいフォルダーにコピーしますか？\n\n新しいフォルダー:\n{dir}\n\n次回起動からこのフォルダーを使用します。", ko: "현재 설정(config.json, 배경 이미지 등)을 새 폴더로 복사할까요?\n\n새 폴더:\n{dir}\n\n다음 실행부터 이 폴더를 사용합니다.", fr: "Copier la configuration actuelle (config.json, images de fond, …) vers le nouveau dossier ?\n\nNouveau dossier :\n{dir}\n\nPinSight utilisera ce dossier au prochain démarrage.", de: "Die aktuelle Konfiguration (config.json, Hintergrundbilder, …) in den neuen Ordner kopieren?\n\nNeuer Ordner:\n{dir}\n\nPinSight verwendet diesen Ordner ab dem nächsten Start." }
    cfg_move_migrate { en: "Copy and switch", zh: "迁移并切换", tw: "遷移並切換", ja: "コピーして切り替え", ko: "복사 후 전환", fr: "Copier et basculer", de: "Kopieren und wechseln" }
    cfg_move_switch { en: "Switch only", zh: "仅切换", tw: "僅切換", ja: "切り替えのみ", ko: "전환만", fr: "Basculer seulement", de: "Nur wechseln" }
    cfg_move_done_title { en: "Config folder changed", zh: "配置文件夹已更改", tw: "設定資料夾已更改", ja: "設定フォルダーを変更しました", ko: "설정 폴더가 변경되었습니다", fr: "Dossier de configuration modifié", de: "Konfigurationsordner geändert" }
    cfg_move_done_body { en: "Restart PinSight so the new folder takes effect.", zh: "请重启 PinSight 使新目录生效。", tw: "請重新啟動 PinSight 使新目錄生效。", ja: "新しいフォルダーを有効にするには PinSight を再起動してください。", ko: "새 폴더를 적용하려면 PinSight을 다시 시작하세요.", fr: "Redémarrez PinSight pour appliquer le nouveau dossier.", de: "Starten Sie PinSight neu, damit der neue Ordner gilt." }
    cfg_move_restart { en: "Restart now", zh: "立即重启", tw: "立即重新啟動", ja: "今すぐ再起動", ko: "지금 재시작", fr: "Redémarrer", de: "Jetzt neu starten" }
    cfg_move_later { en: "Later", zh: "稍后", tw: "稍後", ja: "後で", ko: "나중에", fr: "Plus tard", de: "Später" }
    cfg_move_ok { en: "Moved {n} file(s) to the new folder", zh: "已迁移 {n} 个文件到新目录", tw: "已遷移 {n} 個檔案到新目錄", ja: "{n} 個のファイルを新しいフォルダーへ移動しました", ko: "{n}개 파일을 새 폴더로 옮겼습니다", fr: "{n} fichier(s) déplacé(s) vers le nouveau dossier", de: "{n} Datei(en) in den neuen Ordner verschoben" }
    cfg_move_failed { en: "Could not change the folder", zh: "更改文件夹失败", tw: "更改資料夾失敗", ja: "フォルダーを変更できませんでした", ko: "폴더를 변경하지 못했습니다", fr: "Impossible de changer le dossier", de: "Ordner konnte nicht geändert werden" }
    gen_config { en: "Configuration file", zh: "配置文件位置", tw: "設定檔位置", ja: "設定ファイル", ko: "설정 파일", fr: "Fichier de configuration", de: "Konfigurationsdatei" }
    gen_autostart_saved { en: "Startup entry updated", zh: "开机启动已更新", tw: "開機啟動已更新", ja: "スタートアップを更新しました", ko: "시작 프로그램이 업데이트되었습니다", fr: "Démarrage automatique mis à jour", de: "Autostart aktualisiert" }
    gen_open_folder { en: "Open folder", zh: "打开所在文件夹", tw: "開啟資料夾", ja: "フォルダーを開く", ko: "폴더 열기", fr: "Ouvrir le dossier", de: "Ordner öffnen" }
    tab_appearance { en: "Appearance", zh: "外观", tw: "外觀", ja: "外観", ko: "모양", fr: "Apparence", de: "Aussehen" }
    tab_output { en: "Output", zh: "输出", tw: "輸出", ja: "出力", ko: "출력", fr: "Sortie", de: "Ausgabe" }
    tab_about { en: "Status", zh: "状态", tw: "狀態", ja: "ステータス", ko: "상태", fr: "État", de: "Status" }
    sec_background { en: "Background image", zh: "背景图片", tw: "背景圖片", ja: "背景画像", ko: "배경 이미지", fr: "Image de fond", de: "Hintergrundbild" }
    bg_none { en: "None — flat theme colour", zh: "无 — 纯色主题背景", tw: "無 — 純色主題背景", ja: "なし — 単色テーマ", ko: "없음 — 단색 테마", fr: "Aucune — couleur unie", de: "Keins — einfache Farbe" }
    bg_choose { en: "Choose image…", zh: "选择图片…", tw: "選擇圖片…", ja: "画像を選択…", ko: "이미지 선택…", fr: "Choisir une image…", de: "Bild wählen…" }
    bg_clear { en: "Remove", zh: "移除", tw: "移除", ja: "削除", ko: "제거", fr: "Retirer", de: "Entfernen" }
    bg_blur { en: "Blur", zh: "模糊", tw: "模糊", ja: "ぼかし", ko: "흐림", fr: "Flou", de: "Weichzeichnen" }
    bg_dim { en: "Dimming", zh: "暗化", tw: "暗化", ja: "暗くする", ko: "어둡게", fr: "Assombrir", de: "Abdunkeln" }
    bg_fit { en: "Framing", zh: "填充方式", tw: "填充方式", ja: "表示方法", ko: "맞춤 방식", fr: "Cadrage", de: "Einpassung" }
    bg_cover { en: "Fill (crop)", zh: "填充（裁剪）", tw: "填充（裁切）", ja: "塗りつぶし（切抜き）", ko: "채우기 (자르기)", fr: "Remplir (rogner)", de: "Füllen (beschneiden)" }
    bg_contain { en: "Fit (letterbox)", zh: "适应（留白）", tw: "適應（留白）", ja: "収める（余白）", ko: "맞추기 (여백)", fr: "Ajuster (bandes)", de: "Einpassen (Ränder)" }
    bg_stretch { en: "Stretch", zh: "拉伸", tw: "拉伸", ja: "引き伸ばし", ko: "늘이기", fr: "Étirer", de: "Strecken" }
    bg_load_failed { en: "Could not read that image — background cleared", zh: "无法读取该图片 — 已清除背景", tw: "無法讀取該圖片 — 已清除背景", ja: "その画像を読み込めません — 背景をクリアしました", ko: "이미지를 읽을 수 없습니다 — 배경을 지웠습니다", fr: "Image illisible — fond effacé", de: "Bild nicht lesbar — Hintergrund entfernt" }
    close_to_tray { en: "Close to tray", zh: "关闭时进入托盘", tw: "關閉時進入托盤", ja: "閉じたらトレーへ", ko: "닫으면 트레이로", fr: "Réduire dans la barre", de: "Schließen in die Taskleiste" }
    run_at_startup { en: "Start with the system", zh: "开机自动启动", tw: "開機自動啟動", ja: "システムと同時に起動", ko: "시스템 시작 시 실행", fr: "Démarrer avec le système", de: "Mit dem System starten" }
    startup_failed { en: "The system refused the startup entry — setting reverted", zh: "系统拒绝了启动项 — 已还原设置", tw: "系統拒絕了啟動項 — 已還原設定", ja: "システムが起動項目を拒否しました — 設定を元に戻しました", ko: "시스템이 시작 항목을 거부했습니다 — 설정을 되돌렸습니다", fr: "Le système a refusé l'entrée de démarrage — réglage annulé", de: "Das System hat den Starteintrag abgelehnt — Einstellung zurückgesetzt" }

    // ---- output: naming, image format, clipboard --------------------------
    out_naming { en: "Naming rule", zh: "命名规则", tw: "命名規則", ja: "命名規則", ko: "이름 규칙", fr: "Règle de nommage", de: "Benennungsregel" }
    out_naming_hint { en: "Use $date$ codes and %var% variables. A live preview shows the next file name.", zh: "使用 $日期$ 代码和 %变量%。下方预览显示下一个文件名。", tw: "使用 $日期$ 代碼與 %變數%。下方預覽顯示下一個檔名。", ja: "$日付$ コードと %変数% を使います。下のプレビューに次のファイル名が表示されます。", ko: "$날짜$ 코드와 %변수%를 사용하세요. 아래 미리보기에 다음 파일 이름이 표시됩니다.", fr: "Codes $date$ et variables %var%. L'aperçu montre le prochain nom.", de: "$Datum$-Codes und %Var%-Variablen. Die Vorschau zeigt den nächsten Namen." }
    out_preview { en: "Preview", zh: "预览", tw: "預覽", ja: "プレビュー", ko: "미리보기", fr: "Aperçu", de: "Vorschau" }
    nm_date_codes { en: "Date and time codes ($…$)", zh: "日期和时间代码（$…$）", tw: "日期和時間代碼（$…$）", ja: "日付と時刻のコード（$…$）", ko: "날짜 및 시간 코드（$…$）", fr: "Codes de date et d'heure ($…$)", de: "Datum- und Zeitcodes ($…$)" }
    nm_vars { en: "Special variables (%…%)", zh: "特殊变量（%…%）", tw: "特殊變數（%…%）", ja: "特殊変数（%…%）", ko: "특수 변수(%…%)", fr: "Variables spéciales (%…%)", de: "Spezialvariablen (%…%)" }
    nm_env_hint { en: "…plus any environment variable: %USERNAME%, %COMPUTERNAME%, %USERPROFILE%", zh: "…还支持任意环境变量：%USERNAME%、%COMPUTERNAME%、%USERPROFILE%", tw: "…還支援任意環境變數：%USERNAME%、%COMPUTERNAME%、%USERPROFILE%", ja: "…任意の環境変数も使用できます：%USERNAME%、%COMPUTERNAME%、%USERPROFILE%", ko: "…모든 환경 변수 사용 가능: %USERNAME%, %COMPUTERNAME%, %USERPROFILE%", fr: "…plus toute variable d'environnement", de: "…plus jede Umgebungsvariable" }
    nm_yyyy { en: "year, four digits", zh: "年（四位数）", tw: "年（四位數）", ja: "年（4桁）", ko: "년(4자리)", fr: "année, quatre chiffres", de: "Jahr, vierstellig" }
    nm_yy { en: "year, last two digits", zh: "年（后两位）", tw: "年（後兩位）", ja: "年（下2桁）", ko: "년(뒤 두 자리)", fr: "année, deux chiffres", de: "Jahr, zweistellig" }
    nm_MMMM { en: "month, full name", zh: "月份（全称）", tw: "月份（全名）", ja: "月（フルネーム）", ko: "월(전체 이름)", fr: "mois, nom complet", de: "Monat, vollständig" }
    nm_MMM { en: "month, short name", zh: "月份（缩写）", tw: "月份（縮寫）", ja: "月（略称）", ko: "월(짧은 이름)", fr: "mois, abrégé", de: "Monat, abgekürzt" }
    nm_MM { en: "month, zero-padded", zh: "月份（补零）", tw: "月份（補零）", ja: "月（ゼロ埋め）", ko: "월(0 채움)", fr: "mois, avec zéro", de: "Monat, mit Null" }
    nm_M { en: "month, no padding", zh: "月份（不补零）", tw: "月份（不補零）", ja: "月（ゼロ埋めなし）", ko: "월(채우지 않음)", fr: "mois, sans zéro", de: "Monat, ohne Null" }
    nm_dddd { en: "weekday, full name", zh: "星期（全称）", tw: "星期（全名）", ja: "曜日（フルネーム）", ko: "요일(전체 이름)", fr: "jour, nom complet", de: "Wochentag, vollständig" }
    nm_ddd { en: "weekday, short name", zh: "星期（缩写）", tw: "星期（縮寫）", ja: "曜日（略称）", ko: "요일(짧은 이름)", fr: "jour, abrégé", de: "Wochentag, abgekürzt" }
    nm_dd { en: "day of month, zero-padded", zh: "日（补零）", tw: "日（補零）", ja: "日（ゼロ埋め）", ko: "일(0 채움)", fr: "jour, avec zéro", de: "Tag, mit Null" }
    nm_d { en: "day of month, no padding", zh: "日（不补零）", tw: "日（不補零）", ja: "日（ゼロ埋めなし）", ko: "일(채우지 않음)", fr: "jour, sans zéro", de: "Tag, ohne Null" }
    nm_HH { en: "hour, 24-hour, zero-padded", zh: "时（24 小时制，补零）", tw: "時（24 小時制，補零）", ja: "時（24時間、ゼロ埋め）", ko: "시(24시간, 0 채움)", fr: "heure, 24 h, avec zéro", de: "Stunde, 24 h, mit Null" }
    nm_H { en: "hour, 24-hour, no padding", zh: "时（24 小时制，不补零）", tw: "時（24 小時制，不補零）", ja: "時（24時間、ゼロ埋めなし）", ko: "시(24시간, 채우지 않음)", fr: "heure, 24 h, sans zéro", de: "Stunde, 24 h, ohne Null" }
    nm_hh { en: "hour, 12-hour, zero-padded", zh: "时（12 小时制，补零）", tw: "時（12 小時制，補零）", ja: "時（12時間、ゼロ埋め）", ko: "시(12시간, 0 채움)", fr: "heure, 12 h, avec zéro", de: "Stunde, 12 h, mit Null" }
    nm_h { en: "hour, 12-hour, no padding", zh: "时（12 小时制，不补零）", tw: "時（12 小時制，不補零）", ja: "時（12時間、ゼロ埋めなし）", ko: "시(12시간, 채우지 않음)", fr: "heure, 12 h, sans zéro", de: "Stunde, 12 h, ohne Null" }
    nm_mm { en: "minute, zero-padded", zh: "分（补零）", tw: "分（補零）", ja: "分（ゼロ埋め）", ko: "분(0 채움)", fr: "minute, avec zéro", de: "Minute, mit Null" }
    nm_m { en: "minute, no padding", zh: "分（不补零）", tw: "分（不補零）", ja: "分（ゼロ埋めなし）", ko: "분(채우지 않음)", fr: "minute, sans zéro", de: "Minute, ohne Null" }
    nm_ss { en: "second, zero-padded", zh: "秒（补零）", tw: "秒（補零）", ja: "秒（ゼロ埋め）", ko: "초(0 채움)", fr: "seconde, avec zéro", de: "Sekunde, mit Null" }
    nm_s { en: "second, no padding", zh: "秒（不补零）", tw: "秒（不補零）", ja: "秒（ゼロ埋めなし）", ko: "초(채우지 않음)", fr: "seconde, sans zéro", de: "Sekunde, ohne Null" }
    nm_SSS { en: "millisecond", zh: "毫秒", tw: "毫秒", ja: "ミリ秒", ko: "밀리초", fr: "milliseconde", de: "Millisekunde" }
    nm_A { en: "AM / PM", zh: "上午 / 下午", tw: "上午 / 下午", ja: "午前 / 午後", ko: "오전 / 오후", fr: "AM / PM", de: "AM / PM" }
    nm_os { en: "operating system", zh: "操作系统", tw: "作業系統", ja: "OS 名", ko: "운영 체제", fr: "système d'exploitation", de: "Betriebssystem" }
    nm_computername { en: "computer name", zh: "计算机名", tw: "電腦名稱", ja: "コンピューター名", ko: "컴퓨터 이름", fr: "nom de l'ordinateur", de: "Computername" }
    nm_username { en: "user name", zh: "用户名", tw: "使用者名稱", ja: "ユーザー名", ko: "사용자 이름", fr: "nom d'utilisateur", de: "Benutzername" }
    nm_userprofile { en: "user profile folder", zh: "用户目录", tw: "使用者資料夾", ja: "ユーザープロファイル", ko: "사용자 프로필 폴더", fr: "dossier du profil", de: "Profilordner" }
    nm_title { en: "title of the active window", zh: "当前窗口标题", tw: "目前視窗標題", ja: "アクティブウィンドウのタイトル", ko: "활성 창 제목", fr: "titre de la fenêtre active", de: "Titel des aktiven Fensters" }
    nm_title_cap { en: "window title, cut to 50 characters", zh: "窗口标题（截断到 50 字符）", tw: "視窗標題（截斷至 50 字元）", ja: "ウィンドウタイトル（50 文字）", ko: "창 제목(50자)", fr: "titre, limité à 50 caractères", de: "Fenstertitel, auf 50 Zeichen gekürzt" }
    out_format_codes { en: "Format codes", zh: "格式代码", tw: "格式代碼", ja: "書式コード", ko: "형식 코드", fr: "Codes de format", de: "Formatcodes" }
    out_reset_name { en: "Reset to default", zh: "恢复默认", tw: "回復預設", ja: "既定に戻す", ko: "기본값으로", fr: "Réinitialiser", de: "Zurücksetzen" }
    out_img_format { en: "Image format", zh: "图片格式", tw: "圖片格式", ja: "画像形式", ko: "이미지 형식", fr: "Format d'image", de: "Bildformat" }
    out_img_quality { en: "Quality", zh: "质量", tw: "品質", ja: "品質", ko: "품질", fr: "Qualité", de: "Qualität" }
    out_decorations { en: "Save decorations", zh: "保存后处理", tw: "儲存後處理", ja: "保存時の装飾", ko: "저장 장식", fr: "Décorations d'enregistrement", de: "Speicher-Verzierung" }
    out_decorations_hint { en: "Border and shadow apply to saved captures. The watermark is also burned into recordings.", zh: "边框和阴影应用到保存的截图；水印同时会叠加到录屏画面中。", tw: "邊框和陰影套用到儲存的截圖；浮水印同時會疊加到錄屏畫面中。", ja: "枠と影は保存するキャプチャに、透かしは録画にも適用されます。", ko: "테두리와 그림자는 저장된 캡처에, 워터마크는 녹화 영상에도 적용됩니다.", fr: "Bordure et ombre s'appliquent aux captures ; le filigrane est aussi incrusté dans les enregistrements.", de: "Rahmen und Schatten gelten für Aufnahmen; das Wasserzeichen wird auch in Aufzeichnungen eingebrannt." }
    deco_border { en: "Border", zh: "边框", tw: "邊框", ja: "枠線", ko: "테두리", fr: "Bordure", de: "Rahmen" }
    deco_border_width { en: "Thickness", zh: "边框粗细", tw: "邊框粗細", ja: "太さ", ko: "두께", fr: "Épaisseur", de: "Dicke" }
    deco_border_color { en: "Border colour", zh: "边框颜色", tw: "邊框顏色", ja: "枠線の色", ko: "테두리 색", fr: "Couleur de bordure", de: "Rahmenfarbe" }
    deco_shadow { en: "Drop shadow", zh: "投影", tw: "陰影", ja: "影", ko: "그림자", fr: "Ombre portée", de: "Schatten" }
    deco_shadow_blur { en: "Blur", zh: "模糊", tw: "模糊", ja: "ぼかし", ko: "흐림", fr: "Flou", de: "Weichzeichnen" }
    deco_shadow_offset { en: "Offset", zh: "偏移", tw: "偏移", ja: "オフセット", ko: "오프셋", fr: "Décalage", de: "Versatz" }
    deco_shadow_opacity { en: "Shadow opacity", zh: "阴影浓度", tw: "陰影濃度", ja: "影の濃さ", ko: "그림자 진하기", fr: "Opacité de l'ombre", de: "Schattenstärke" }
    deco_shadow_color { en: "Shadow colour", zh: "阴影颜色", tw: "陰影顏色", ja: "影の色", ko: "그림자 색", fr: "Couleur de l'ombre", de: "Schattenfarbe" }
    deco_watermark { en: "Watermark", zh: "水印", tw: "浮水印", ja: "透かし", ko: "워터마크", fr: "Filigrane", de: "Wasserzeichen" }
    deco_wm_text { en: "Text", zh: "文字", tw: "文字", ja: "テキスト", ko: "텍스트", fr: "Texte", de: "Text" }
    deco_wm_position { en: "Position", zh: "位置", tw: "位置", ja: "位置", ko: "위치", fr: "Position", de: "Position" }
    deco_wm_tile { en: "Tile", zh: "平铺", tw: "平鋪", ja: "タイル", ko: "타일", fr: "Mosaïque", de: "Kacheln" }
    deco_wm_center { en: "Centre", zh: "居中", tw: "置中", ja: "中央", ko: "가운데", fr: "Centre", de: "Mitte" }
    deco_wm_tl { en: "Top left", zh: "左上", tw: "左上", ja: "左上", ko: "왼쪽 위", fr: "Haut gauche", de: "Oben links" }
    deco_wm_tr { en: "Top right", zh: "右上", tw: "右上", ja: "右上", ko: "오른쪽 위", fr: "Haut droite", de: "Oben rechts" }
    deco_wm_bl { en: "Bottom left", zh: "左下", tw: "左下", ja: "左下", ko: "왼쪽 아래", fr: "Bas gauche", de: "Unten links" }
    deco_wm_br { en: "Bottom right", zh: "右下", tw: "右下", ja: "右下", ko: "오른쪽 아래", fr: "Bas droite", de: "Unten rechts" }
    deco_wm_opacity { en: "Watermark opacity", zh: "水印浓度", tw: "浮水印濃度", ja: "透かしの濃さ", ko: "워터마크 진하기", fr: "Opacité du filigrane", de: "Wasserzeichenstärke" }
    deco_wm_size { en: "Text size", zh: "文字大小", tw: "文字大小", ja: "文字サイズ", ko: "글자 크기", fr: "Taille du texte", de: "Textgröße" }
    deco_wm_margin { en: "Margin", zh: "边距", tw: "邊距", ja: "余白", ko: "여백", fr: "Marge", de: "Abstand" }
    deco_wm_image { en: "Image mark", zh: "图片水印", tw: "圖片浮水印", ja: "画像マーク", ko: "이미지 워터마크", fr: "Filigrane image", de: "Bild-Wasserzeichen" }
    deco_wm_image_hint { en: "Path to an image, optional", zh: "图片路径（可选）", tw: "圖片路徑（選填）", ja: "画像パス（任意）", ko: "이미지 경로(선택)", fr: "Chemin d'une image (facultatif)", de: "Bildpfad (optional)" }
    deco_wm_browse { en: "Choose…", zh: "选择…", tw: "選擇…", ja: "選択…", ko: "선택…", fr: "Choisir…", de: "Wählen…" }
    deco_rounded { en: "Rounded corners", zh: "圆角", tw: "圓角", ja: "角丸", ko: "둥근 모서리", fr: "Coins arrondis", de: "Abgerundete Ecken" }
    deco_rounded_radius { en: "Corner radius", zh: "圆角半径", tw: "圓角半徑", ja: "角丸半径", ko: "모서리 반경", fr: "Rayon des coins", de: "Eckenradius" }
    rec_settings { en: "Recording", zh: "录屏", tw: "錄屏", ja: "録画", ko: "녹화", fr: "Enregistrement", de: "Aufnahme" }
    rec_fps { en: "Frame rate", zh: "帧率", tw: "影格率", ja: "フレームレート", ko: "프레임률", fr: "Images par seconde", de: "Bildrate" }
    out_clipboard { en: "Clipboard", zh: "复制到剪贴板", tw: "複製到剪貼簿", ja: "クリップボードにコピー", ko: "클립보드에 복사", fr: "Presse-papiers", de: "Zwischenablage" }
    out_clip_image { en: "Image", zh: "图片", tw: "圖片", ja: "画像", ko: "이미지", fr: "Image", de: "Bild" }
    out_clip_file { en: "Copy as file", zh: "作为文件复制", tw: "作為檔案複製", ja: "ファイルとしてコピー", ko: "파일로 복사", fr: "Comme fichier", de: "Als Datei" }
    out_clip_disabled { en: "Do not copy", zh: "不复制", tw: "不複製", ja: "コピーしない", ko: "복사 안 함", fr: "Ne pas copier", de: "Nicht kopieren" }
    out_clip_image_hint { en: "A PNG and a Windows bitmap (DIB) are placed on the clipboard, so both modern apps and Office accept it.", zh: "会同时写入 PNG 和 Windows 位图（DIB），现代应用和 Office 都能粘贴。", tw: "會同時寫入 PNG 與 Windows 點陣圖（DIB），現代應用與 Office 都能貼上。", ja: "PNG と Windows ビットマップ (DIB) の両方を入れ、モダンなアプリと Office の両方で貼り付けられます。", ko: "PNG과 Windows 비트맵(DIB)을 모두 넣어 최신 앱과 Office에서 모두 붙여넣기 가능.", fr: "Un PNG et un bitmap Windows (DIB) sont copiés : applications modernes et Office acceptent.", de: "PNG und Windows-Bitmap (DIB) landen in der Zwischenablage — moderne Apps und Office." }

    // ---- upload settings --------------------------------------------------
    up_section { en: "Upload", zh: "上传", tw: "上傳", ja: "アップロード", ko: "업로드", fr: "Envoi", de: "Hochladen" }
    up_enabled { en: "Enable upload", zh: "启用上传", tw: "啟用上傳", ja: "アップロードを有効化", ko: "업로드 사용", fr: "Activer l'envoi", de: "Hochladen aktivieren" }
    up_backend { en: "Backend", zh: "后端", tw: "後端", ja: "バックエンド", ko: "백엔드", fr: "Backend", de: "Backend" }
    up_http { en: "HTTP", zh: "HTTP", tw: "HTTP", ja: "HTTP", ko: "HTTP", fr: "HTTP", de: "HTTP" }
    up_local { en: "Local folder", zh: "本地文件夹", tw: "本機資料夾", ja: "ローカルフォルダー", ko: "로컬 폴더", fr: "Dossier local", de: "Lokaler Ordner" }
    up_url { en: "Upload URL", zh: "上传地址", tw: "上傳位址", ja: "アップロード URL", ko: "업로드 URL", fr: "URL d'envoi", de: "Upload-URL" }
    up_method { en: "Method", zh: "方法", tw: "方法", ja: "メソッド", ko: "메서드", fr: "Méthode", de: "Methode" }
    up_field { en: "Form field", zh: "表单字段", tw: "表單欄位", ja: "フォーム項目", ko: "폼 필드", fr: "Champ de formulaire", de: "Formularfeld" }
    up_response { en: "Response", zh: "响应", tw: "回應", ja: "レスポンス", ko: "응답", fr: "Réponse", de: "Antwort" }
    up_resp_raw { en: "Raw body", zh: "原始响应", tw: "原始回應", ja: "生ボディ", ko: "원시 본문", fr: "Corps brut", de: "Rohtext" }
    up_resp_json { en: "JSON field", zh: "JSON 字段", tw: "JSON 欄位", ja: "JSON 項目", ko: "JSON 필드", fr: "Champ JSON", de: "JSON-Feld" }
    up_json_field { en: "JSON field name", zh: "JSON 字段名", tw: "JSON 欄位名", ja: "JSON 項目名", ko: "JSON 필드 이름", fr: "Nom du champ JSON", de: "JSON-Feldname" }
    up_folder { en: "Folder", zh: "文件夹", tw: "資料夾", ja: "フォルダー", ko: "폴더", fr: "Dossier", de: "Ordner" }
    up_hint { en: "Upload posts the PNG to your endpoint, or copies it to a local folder. Nothing leaves the machine unless you click Upload.", zh: "上传会把 PNG 发到你的接口，或复制到本地文件夹。只有你点“上传”时才会动。", tw: "上傳會把 PNG 傳到你的介面，或複製到本機資料夾。只有你按「上傳」時才會動作。", ja: "PNG を指定エンドポイントへ送信、またはローカルフォルダーへコピーします。アップロードを押した時だけ実行されます。", ko: "PNG를 지정 엔드포인트로 보내거나 로컬 폴더로 복사합니다. 업로드를 누를 때만 동작합니다.", fr: "L'envoi poste le PNG vers votre point d'accès ou le copie dans un dossier local. Rien ne part sans clic sur Envoyer.", de: "Hochladen sendet das PNG an deinen Endpunkt oder kopiert es in einen lokalen Ordner. Nur bei Klick auf Hochladen." }

    // ---- tray expansion ---------------------------------------------------
    tray_toggle_pins_show { en: "Show all pins", zh: "显示所有贴图", tw: "顯示所有貼圖", ja: "すべてのピンを表示", ko: "모든 핀 표시", fr: "Afficher les épingles", de: "Alle Pins zeigen" }
    tray_toggle_pins_hide { en: "Hide all pins", zh: "隐藏所有贴图", tw: "隱藏所有貼圖", ja: "すべてのピンを隠す", ko: "모든 핀 숨기기", fr: "Masquer les épingles", de: "Alle Pins verbergen" }
    tray_disable_hk { en: "Disable hotkeys", zh: "禁用截图快捷键", tw: "停用截圖快速鍵", ja: "ショートカットを無効化", ko: "단축키 비활성화", fr: "Désactiver les raccourcis", de: "Tastenkürzel deaktivieren" }
    tray_enable_hk { en: "Enable hotkeys", zh: "启用截图快捷键", tw: "啟用截圖快速鍵", ja: "ショートカットを有効化", ko: "단축키 활성화", fr: "Activer les raccourcis", de: "Tastenkürzel aktivieren" }
    tray_check_update { en: "Check for updates", zh: "检查更新", tw: "檢查更新", ja: "更新を確認", ko: "업데이트 확인", fr: "Vérifier les mises à jour", de: "Nach Updates suchen" }
    tray_restart { en: "Restart PinSight", zh: "重新启动", tw: "重新啟動", ja: "PinSight を再起動", ko: "PinSight 다시 시작", fr: "Redémarrer PinSight", de: "PinSight neu starten" }
    tray_clear_history { en: "Clear capture history", zh: "清空截图历史", tw: "清空截圖歷史", ja: "撮影履歴を消去", ko: "캡처 기록 지우기", fr: "Effacer l'historique des captures", de: "Aufnahmeverlauf löschen" }
    tray_pins_submenu { en: "Pins", zh: "贴图", tw: "貼圖", ja: "ピン", ko: "핀", fr: "Épingles", de: "Pins" }
    tray_no_pins { en: "(no pins yet)", zh: "（暂无贴图）", tw: "（暫無貼圖）", ja: "（ピンがありません）", ko: "(핀 없음)", fr: "(aucune épingle)", de: "(keine Pins)" }
    tray_help { en: "Help", zh: "帮助", tw: "說明", ja: "ヘルプ", ko: "도움말", fr: "Aide", de: "Hilfe" }
    tray_prefs { en: "Preferences…", zh: "首选项…", tw: "偏好設定…", ja: "設定…", ko: "환경설정…", fr: "Préférences…", de: "Einstellungen…" }
    tray_pins_hidden_toast { en: "All pins hidden", zh: "已隐藏所有贴图", tw: "已隱藏所有貼圖", ja: "すべてのピンを非表示にしました", ko: "모든 핀을 숨겼습니다", fr: "Toutes les épingles sont masquées", de: "Alle Pins ausgeblendet" }
    tray_pins_shown_toast { en: "All pins shown", zh: "已显示所有贴图", tw: "已顯示所有貼圖", ja: "すべてのピンを表示しました", ko: "모든 핀을 표시했습니다", fr: "Toutes les épingles sont affichées", de: "Alle Pins eingeblendet" }
    tray_hotkeys_off_toast { en: "Hotkeys disabled", zh: "已禁用快捷键", tw: "已停用快速鍵", ja: "ショートカットを無効化しました", ko: "단축키를 비활성화했습니다", fr: "Raccourcis désactivés", de: "Tastenkürzel deaktiviert" }
    tray_hotkeys_on_toast { en: "Hotkeys enabled", zh: "已启用快捷键", tw: "已啟用快速鍵", ja: "ショートカットを有効化しました", ko: "단축키를 활성화했습니다", fr: "Raccourcis activés", de: "Tastenkürzel aktiviert" }
    tray_history_cleared_toast { en: "Capture history cleared", zh: "已清空截图历史", tw: "已清空截圖歷史", ja: "撮影履歴を消去しました", ko: "캡처 기록을 지웠습니다", fr: "Historique effacé", de: "Verlauf gelöscht" }
    tray_restarting_toast { en: "Restarting PinSight…", zh: "正在重启 PinSight…", tw: "正在重新啟動 PinSight…", ja: "PinSight を再起動します…", ko: "PinSight를 다시 시작합니다…", fr: "Redémarrage de PinSight…", de: "PinSight wird neu gestartet…" }
    tray_no_history_toast { en: "There is no capture history yet", zh: "还没有截图历史", tw: "還沒有截圖歷史", ja: "撮影履歴はまだありません", ko: "캡처 기록이 아직 없습니다", fr: "Aucun historique de capture", de: "Noch kein Aufnahmeverlauf" }

    // ---- pins subtabs -----------------------------------------------------
    pin_sub_display { en: "Display", zh: "显示", tw: "顯示", ja: "表示", ko: "표시", fr: "Affichage", de: "Anzeige" }
    pin_sub_behavior { en: "Behavior", zh: "行为", tw: "行為", ja: "動作", ko: "동작", fr: "Comportement", de: "Verhalten" }
    pin_sub_convert { en: "Convert", zh: "转换", tw: "轉換", ja: "変換", ko: "변환", fr: "Conversion", de: "Konvertieren" }
    pin_wheel_zoom { en: "Mouse wheel zooms the pin", zh: "鼠标滚轮缩放贴图", tw: "滑鼠滾輪縮放貼圖", ja: "マウスホイールでピンを拡大縮小", ko: "마우스 휠로 핀 확대/축소", fr: "La molette zoome l'épingle", de: "Mausrad zoomt die Pin" }
    pin_middle_close { en: "Middle click closes the pin", zh: "中键单击关闭贴图", tw: "中鍵單擊關閉貼圖", ja: "中クリックでピンを閉じる", ko: "중간 클릭으로 핀 닫기", fr: "Le clic du milieu ferme l'épingle", de: "Mittelklick schließt die Pin" }
    pin_keep_position { en: "Reuse the capture position", zh: "沿用截图时的位置", tw: "沿用截圖時的位置", ja: "撮影時の位置を再利用する", ko: "캡처 위치를 그대로 사용", fr: "Réutiliser la position de la capture", de: "Position der Aufnahme übernehmen" }
    pin_text_to_image { en: "Convert text to an image", zh: "文字转图片", tw: "文字轉圖片", ja: "文字を画像に変換", ko: "텍스트를 이미지로 변환", fr: "Convertir le texte en image", de: "Text in ein Bild umwandeln" }
    pin_text_margin { en: "Page margin", zh: "页边距", tw: "頁邊距", ja: "余白", ko: "여백", fr: "Marge de page", de: "Seitenrand" }
    pin_text_max_width { en: "Maximum image width", zh: "图片最大宽度", tw: "圖片最大寬度", ja: "画像の最大幅", ko: "이미지 최대 너비", fr: "Largeur maximale", de: "Maximale Bildbreite" }
    pin_text_to_image_hint { en: "Text on the clipboard is turned into an image with the UI font and the margins above.", zh: "剪贴板里的文字会用界面字体和上面的边距渲染成图片。", tw: "剪貼簿裡的文字會用介面字型與上述邊距轉成圖片。", ja: "クリップボードのテキストを、上記の余白と UI フォントで画像にします。", ko: "클립보드의 텍스트를 위 여백과 UI 글꼴로 이미지로 만듭니다.", fr: "Le texte du presse-papiers est transformé en image avec la police de l'interface.", de: "Text aus der Zwischenablage wird mit der UI-Schrift in ein Bild umgewandelt." }
    pin_behavior_note { en: "Wheel zooms, middle-click closes. Changes apply to pins opened afterwards.", zh: "滚轮缩放、中键关闭。改动对之后打开的贴图生效。", tw: "滾輪縮放、中鍵關閉。變更對之後開啟的貼圖生效。", ja: "ホイールで拡大縮小、中クリックで閉じます。変更は以降に開いたピンに適用されます。", ko: "휠로 확대/축소, 가운데 클릭으로 닫기. 변경은 이후에 연 핀에 적용됩니다.", fr: "Molette pour zoomer, clic du milieu pour fermer. Les changements s'appliquent aux épingles ouvertes ensuite.", de: "Mausrad zoomt, Mittelklick schließt. Änderungen gelten für danach geöffnete Pins." }


    // ---- update proxy -----------------------------------------------------
    upd_proxy { en: "Update proxy", zh: "更新代理", tw: "更新代理", ja: "更新プロキシ", ko: "업데이트 프록시", fr: "Proxy de mise à jour", de: "Update-Proxy" }
    upd_proxy_mode { en: "Proxy type", zh: "代理类型", tw: "代理類型", ja: "プロキシの種類", ko: "프록시 종류", fr: "Type de proxy", de: "Proxy-Art" }
    upd_proxy_system { en: "Use system proxy", zh: "使用系统代理", tw: "使用系統代理", ja: "システムプロキシを使用", ko: "시스템 프록시 사용", fr: "Proxy système", de: "System-Proxy" }
    upd_proxy_none { en: "No proxy", zh: "不使用代理", tw: "不使用代理", ja: "プロキシなし", ko: "프록시 없음", fr: "Aucun proxy", de: "Kein Proxy" }
    upd_proxy_manual { en: "Manual", zh: "手动", tw: "手動", ja: "手動", ko: "수동", fr: "Manuel", de: "Manuell" }
    upd_proxy_addr { en: "Server", zh: "服务器", tw: "伺服器", ja: "サーバー", ko: "서버", fr: "Serveur", de: "Server" }
    upd_proxy_user { en: "User name", zh: "用户名", tw: "使用者名稱", ja: "ユーザー名", ko: "사용자 이름", fr: "Nom d'utilisateur", de: "Benutzername" }
    upd_proxy_port { en: "Port", zh: "端口", tw: "連接埠", ja: "ポート", ko: "포트", fr: "Port", de: "Port" }
    upd_proxy_pass { en: "Password", zh: "密码", tw: "密碼", ja: "パスワード", ko: "비밀번호", fr: "Mot de passe", de: "Passwort" }
    upd_proxy_note { en: "Used for the update check and download.", zh: "用于检查更新和下载安装包。", tw: "用於檢查更新與下載安裝包。", ja: "更新の確認とダウンロードに使用します。", ko: "업데이트 확인 및 다운로드에 사용됩니다.", fr: "Utilisé pour la vérification et le téléchargement des mises à jour.", de: "Wird für Update-Prüfung und -Download verwendet." }

    // ---- tray -------------------------------------------------------------
    tray_show { en: "Open PinSight", zh: "打开 PinSight", tw: "開啟 PinSight", ja: "PinSight を開く", ko: "PinSight 열기", fr: "Ouvrir PinSight", de: "PinSight öffnen" }
    tray_capture_area { en: "Capture area", zh: "区域截图", tw: "區域截圖", ja: "範囲を撮影", ko: "영역 캡처", fr: "Capturer une zone", de: "Bereich aufnehmen" }
    tray_capture_full { en: "Full screen", zh: "全屏截图", tw: "全螢幕截圖", ja: "全画面", ko: "전체 화면", fr: "Plein écran", de: "Ganzer Bildschirm" }
    tray_pin_clipboard { en: "Pin clipboard", zh: "贴图剪贴板", tw: "貼圖剪貼簿", ja: "クリップボード固定", ko: "클립보드 고정", fr: "Épingler le presse-papiers", de: "Zwischenablage anheften" }
    tray_long_capture { en: "Scroll capture", zh: "长截图", tw: "長截圖", ja: "スクロール撮影", ko: "스크롤 캡처", fr: "Capture défilante", de: "Scroll-Aufnahme" }
    tray_record { en: "Record area", zh: "区域录屏", tw: "區域錄影", ja: "範囲を録画", ko: "영역 녹화", fr: "Enregistrer une zone", de: "Bereich aufzeichnen" }
    tray_save_last { en: "Save last", zh: "保存最近截图", tw: "儲存最近截圖", ja: "直前の撮影を保存", ko: "최근 캡처 저장", fr: "Enregistrer la dernière", de: "Letzte sichern" }
    tray_quit { en: "Quit", zh: "退出", tw: "結束", ja: "終了", ko: "종료", fr: "Quitter", de: "Beenden" }
    tray_tip { en: "PinSight — capture, pin, record", zh: "PinSight — 截图 · 贴图 · 录屏", tw: "PinSight — 截圖 · 貼圖 · 錄影", ja: "PinSight — 撮影 · ピン · 録画", ko: "PinSight — 캡처 · 핀 · 녹화", fr: "PinSight — capture, épingle, enregistrement", de: "PinSight — aufnehmen, anheften, aufzeichnen" }

    // ---- hotkey recorder --------------------------------------------------
    hk_press { en: "Record", zh: "录制", tw: "錄製", ja: "録音", ko: "녹음", fr: "Enregistrer", de: "Aufnehmen" }
    hk_press_prompt { en: "Press the new combination… (Esc to cancel)", zh: "请按下新的组合键…（Esc 取消）", tw: "請按下新的組合鍵…（Esc 取消）", ja: "新しい組み合わせを押してください…（Esc で中止）", ko: "새 조합을 누르세요… (Esc 취소)", fr: "Appuyez sur la nouvelle combinaison… (Échap annule)", de: "Neue Kombination drücken… (Esc bricht ab)" }
    hk_rejected { en: "That combination needs at least one modifier (Ctrl / Alt / Shift / Win)", zh: "该组合需要至少一个修饰键（Ctrl / Alt / Shift / Win）", tw: "該組合需要至少一個修飾鍵（Ctrl / Alt / Shift / Win）", ja: "修飾キー（Ctrl / Alt / Shift / Win）が最低 1 つ必要です", ko: "수정 키(Ctrl / Alt / Shift / Win)가 하나 이상 필요합니다", fr: "La combinaison exige un modificateur (Ctrl / Alt / Shift / Win)", de: "Die Kombination braucht mindestens einen Modifikator (Ctrl / Alt / Shift / Win)" }
    hk_duplicate { en: "That combination is already used by another action", zh: "该组合已被另一个动作占用", tw: "該組合已被另一個動作佔用", ja: "この組み合わせは他の操作で使用されています", ko: "이 조합은 다른 동작에서 이미 사용 중입니다", fr: "Cette combinaison est déjà utilisée par une autre action", de: "Diese Kombination wird bereits von einer anderen Aktion verwendet" }
    hk_cancelled { en: "Recording cancelled", zh: "已取消录制", tw: "已取消錄製", ja: "録音を中止しました", ko: "녹음을 취소했습니다", fr: "Enregistrement annulé", de: "Aufnahme abgebrochen" }

    // ---- toasts -----------------------------------------------------------
    ready { en: "Ready — press the area shortcut to capture", zh: "已就绪 — 按区域截图快捷键开始", tw: "已就緒 — 按區域截圖快速鍵開始", ja: "準備完了 — 範囲撮影のショートカットで開始", ko: "준비 완료 — 영역 캡처 단축키로 시작", fr: "Prêt — utilisez le raccourci de zone", de: "Bereit — Bereichs-Shortcut drücken" }
    hotkey_warning { en: "Hotkey warning", zh: "快捷键警告", tw: "快速鍵警告", ja: "ショートカットの警告", ko: "단축키 경고", fr: "Avertissement raccourci", de: "Tastenkürzel-Warnung" }
    hotkey_in_use { en: "already used by another app", zh: "已被其他程序占用", tw: "已被其他程式佔用", ja: "他のアプリが使用中です", ko: "다른 앱이 사용 중입니다", fr: "déjà utilisée par une autre application", de: "bereits von einer anderen App belegt" }
    hotkey_invalid { en: "not a valid shortcut", zh: "不是有效的快捷键", tw: "不是有效的快速鍵", ja: "無効なショートカットです", ko: "올바른 단축키가 아닙니다", fr: "raccourci invalide", de: "kein gültiges Kürzel" }
    hotkey_unavailable { en: "Shortcut unavailable", zh: "快捷键不可用", tw: "快速鍵無法使用", ja: "ショートカットを使用できません", ko: "단축키를 사용할 수 없습니다", fr: "Raccourci indisponible", de: "Kürzel nicht verfügbar" }
    hotkeys_conflict_hint { en: "Shortcuts marked in red could not be registered. Pick another combination and apply.", zh: "标红的快捷键未能注册，请换一个组合后重新应用。", tw: "標紅的快速鍵未能註冊，請換一個組合後重新套用。", ja: "赤い項目は登録できませんでした。別の組み合わせで再適用してください。", ko: "빨간색 단축키는 등록되지 않았습니다. 다른 조합으로 다시 적용하세요.", fr: "Les raccourcis en rouge n'ont pas pu être enregistrés. Choisissez une autre combinaison.", de: "Rot markierte Kürzel konnten nicht registriert werden. Andere Kombination wählen." }
    capture_in_progress { en: "A capture is already in progress", zh: "已有截图任务在进行", tw: "已有截圖工作正在進行", ja: "すでに撮影中です", ko: "이미 캡처가 진행 중입니다", fr: "Une capture est déjà en cours", de: "Es läuft bereits eine Aufnahme" }
    capture_failed { en: "Capture failed", zh: "截图失败", tw: "截圖失敗", ja: "撮影に失敗しました", ko: "캡처 실패", fr: "Échec de la capture", de: "Aufnahme fehlgeschlagen" }
    clipboard_error { en: "Clipboard error", zh: "剪贴板错误", tw: "剪貼簿錯誤", ja: "クリップボードのエラー", ko: "클립보드 오류", fr: "Erreur du presse-papiers", de: "Zwischenablage-Fehler" }
    clipboard_pinned { en: "Clipboard image pinned", zh: "剪贴板图片已贴图", tw: "剪貼簿圖片已貼圖", ja: "クリップボード画像を固定しました", ko: "클립보드 이미지를 고정했습니다", fr: "Image du presse-papiers épinglée", de: "Bild aus der Zwischenablage angeheftet" }
    clipboard_no_image { en: "The clipboard has no image", zh: "剪贴板里没有图片", tw: "剪貼簿裡沒有圖片", ja: "クリップボードに画像がありません", ko: "클립보드에 이미지가 없습니다", fr: "Le presse-papiers ne contient aucune image", de: "Die Zwischenablage enthält kein Bild" }
    copied { en: "Copied to clipboard", zh: "已复制到剪贴板", tw: "已複製到剪貼簿", ja: "クリップボードにコピーしました", ko: "클립보드에 복사했습니다", fr: "Copié dans le presse-papiers", de: "In die Zwischenablage kopiert" }
    uploading { en: "Uploading…", zh: "上传中…", tw: "上傳中…", ja: "アップロード中…", ko: "업로드 중…", fr: "Envoi…", de: "Lade hoch…" }
    uploaded { en: "Uploaded — link copied", zh: "已上传，链接已复制", tw: "已上傳，連結已複製", ja: "アップロード完了、リンクをコピーしました", ko: "업로드 완료, 링크 복사됨", fr: "Envoyé — lien copié", de: "Hochgeladen — Link kopiert" }
    upload_failed { en: "Upload failed", zh: "上传失败", tw: "上傳失敗", ja: "アップロード失敗", ko: "업로드 실패", fr: "Échec de l'envoi", de: "Hochladen fehlgeschlagen" }
    copy_failed { en: "Copy failed", zh: "复制失败", tw: "複製失敗", ja: "コピーに失敗しました", ko: "복사 실패", fr: "Échec de la copie", de: "Kopieren fehlgeschlagen" }
    pinned { en: "Pinned", zh: "已贴图", tw: "已貼圖", ja: "固定しました", ko: "고정했습니다", fr: "Épinglé", de: "Angeheftet" }
    saved { en: "Saved", zh: "已保存", tw: "已儲存", ja: "保存しました", ko: "저장했습니다", fr: "Enregistré", de: "Gespeichert" }
    save_failed { en: "Save failed", zh: "保存失败", tw: "儲存失敗", ja: "保存に失敗しました", ko: "저장 실패", fr: "Échec de l'enregistrement", de: "Speichern fehlgeschlagen" }
    nothing_to_save { en: "Nothing to save yet", zh: "还没有可保存的截图", tw: "還沒有可儲存的截圖", ja: "保存できる撮影がありません", ko: "저장할 캡처가 없습니다", fr: "Rien à enregistrer", de: "Noch nichts zu speichern" }
    stopping_recording { en: "Finishing the recording…", zh: "正在结束录制…", tw: "正在結束錄影…", ja: "録画を終了しています…", ko: "녹화를 마무리하는 중…", fr: "Fin de l'enregistrement…", de: "Aufzeichnung wird beendet…" }
    long_started { en: "Scroll capture started — keep the window visible", zh: "长截图已开始 — 请保持窗口可见", tw: "長截圖已開始 — 請保持視窗可見", ja: "スクロール撮影を開始 — ウィンドウを表示したままに", ko: "스크롤 캡처 시작 — 창을 계속 보이게 두세요", fr: "Capture défilante lancée — gardez la fenêtre visible", de: "Scroll-Aufnahme gestartet — Fenster sichtbar lassen" }
    long_progress { en: "Scroll capture running", zh: "长截图进行中", tw: "長截圖進行中", ja: "スクロール撮影を実行中", ko: "스크롤 캡처 진행 중", fr: "Capture défilante en cours", de: "Scroll-Aufnahme läuft" }
    long_manual_started { en: "Manual scroll capture — roll the wheel to capture, Esc to finish", zh: "手动长截图 — 滚动鼠标滚轮截取，Esc 完成", tw: "手動長截圖 — 滾動滑鼠滾輪截取，Esc 完成", ja: "手動長截圖 — ホイールで截取、Esc で完了", ko: "수동 스크롤 캡처 — 휠로 캡처, Esc 로 완료", fr: "Capture manuelle — molette pour capturer, Échap pour terminer", de: "Manuelle Aufnahme — Mausrad zum Erfassen, Esc zum Beenden" }
    long_cancelled { en: "Cancelling scroll capture…", zh: "正在取消长截图…", tw: "正在取消長截圖…", ja: "スクロール撮影を中止しています…", ko: "스크롤 캡처를 취소하는 중…", fr: "Annulation de la capture défilante…", de: "Scroll-Aufnahme wird abgebrochen…" }
    long_failed { en: "Scroll capture failed", zh: "长截图失败", tw: "長截圖失敗", ja: "スクロール撮影に失敗しました", ko: "스크롤 캡처 실패", fr: "Échec de la capture défilante", de: "Scroll-Aufnahme fehlgeschlagen" }
    long_saved { en: "Scroll capture saved", zh: "长截图已保存", tw: "長截圖已儲存", ja: "スクロール撮影を保存しました", ko: "스크롤 캡처를 저장했습니다", fr: "Capture défilante enregistrée", de: "Scroll-Aufnahme gespeichert" }
    long_truncated { en: "Limit reached — image may be incomplete", zh: "已达上限，图片可能不完整", tw: "已達上限，圖片可能不完整", ja: "上限に達しました — 画像が不完全な可能性があります", ko: "한도 도달 — 이미지가 불완전할 수 있습니다", fr: "Limite atteinte — l'image peut être incomplète", de: "Grenze erreicht — Bild möglicherweise unvollständig" }
    pin_create_failed { en: "Could not create the pin", zh: "无法创建贴图窗口", tw: "無法建立貼圖視窗", ja: "ピンを作成できませんでした", ko: "핀을 만들 수 없습니다", fr: "Impossible de créer l'épingle", de: "Anheften fehlgeschlagen" }
    embed_unsupported { en: "This compositor does not support overlay windows", zh: "当前合成器不支持浮层窗口", tw: "目前合成器不支援浮層視窗", ja: "このコンポジターはオーバーレイをサポートしません", ko: "이 컴포지터는 오버레이 창을 지원하지 않습니다", fr: "Ce compositeur ne gère pas les fenêtres superposées", de: "Dieser Compositor unterstützt keine Overlay-Fenster" }
    already_recording { en: "Already recording", zh: "正在录制中", tw: "正在錄影中", ja: "すでに録画中です", ko: "이미 녹화 중입니다", fr: "Enregistrement déjà en cours", de: "Aufzeichnung läuft bereits" }
    hotkeys_applied { en: "Hotkeys updated", zh: "快捷键已更新", tw: "快速鍵已更新", ja: "ショートカットを更新しました", ko: "단축키를 업데이트했습니다", fr: "Raccourcis mis à jour", de: "Tastenkürzel aktualisiert" }
    font_loaded { en: "Font applied", zh: "字体已应用", tw: "字型已套用", ja: "フォントを適用しました", ko: "글꼴을 적용했습니다", fr: "Police appliquée", de: "Schrift angewendet" }
    font_missing { en: "Font file not found — keeping the current font", zh: "找不到字体文件 — 保持当前字体", tw: "找不到字型檔案 — 保持目前字型", ja: "フォントが見つかりません — 現在の設定を維持", ko: "글꼴 파일을 찾을 수 없습니다 — 현재 글꼴 유지", fr: "Police introuvable — police actuelle conservée", de: "Schriftdatei fehlt — aktuelle Schrift bleibt" }

    // ---- capture overlay --------------------------------------------------
    ov_title { en: "PinSight — capture", zh: "PinSight — 截图", tw: "PinSight — 截圖", ja: "PinSight — キャプチャ", ko: "PinSight — 캡처", fr: "PinSight — capture", de: "PinSight — Aufnahme" }
    ov_hint_pick { en: "Click to sample the colour · Esc to cancel", zh: "单击取色 · Esc 取消", tw: "單擊取色 · Esc 取消", ja: "クリックで色を取得 · Esc で中止", ko: "클릭하여 색 추출 · Esc 취소", fr: "Cliquez pour prélever · Échap annuler", de: "Klicken zum Aufnehmen · Esc abbrechen" }
    color_pick_title { en: "Colour", zh: "选择颜色", tw: "選擇顏色", ja: "色の選択", ko: "색 선택", fr: "Couleur", de: "Farbe" }
    color_basic { en: "Basic colours", zh: "基本颜色", tw: "基本顏色", ja: "基本色", ko: "기본 색", fr: "Couleurs de base", de: "Grundfarben" }
    color_custom { en: "Custom colours", zh: "自定义颜色", tw: "自訂顏色", ja: "ユーザー設定色", ko: "사용자 지정 색", fr: "Couleurs personnalisées", de: "Benutzerdefiniert" }
    color_add_custom { en: "Add to custom colours", zh: "添加到自定义颜色", tw: "新增至自訂顏色", ja: "ユーザー設定色に追加", ko: "사용자 지정 색에 추가", fr: "Ajouter aux couleurs personnalisées", de: "Zu benutzerdefiniert hinzufügen" }
    color_pick_screen { en: "Pick from screen", zh: "屏幕取色", tw: "螢幕取色", ja: "画面から取得", ko: "화면에서 추출", fr: "Prélever à l'écran", de: "Vom Bildschirm" }
    color_hue { en: "Hue", zh: "色调", tw: "色調", ja: "色相", ko: "색조", fr: "Teinte", de: "Farbton" }
    color_sat { en: "Saturation", zh: "饱和度", tw: "飽和度", ja: "彩度", ko: "채도", fr: "Saturation", de: "Sättigung" }
    color_val { en: "Brightness", zh: "亮度", tw: "亮度", ja: "明度", ko: "밝기", fr: "Luminosité", de: "Helligkeit" }
    color_red { en: "Red", zh: "红色", tw: "紅色", ja: "赤", ko: "빨강", fr: "Rouge", de: "Rot" }
    color_green { en: "Green", zh: "绿色", tw: "綠色", ja: "緑", ko: "초록", fr: "Vert", de: "Grün" }
    color_blue { en: "Blue", zh: "蓝色", tw: "藍色", ja: "青", ko: "파랑", fr: "Bleu", de: "Blau" }
    color_alpha { en: "Alpha", zh: "Alpha 通道", tw: "Alpha 通道", ja: "アルファ", ko: "알파", fr: "Alpha", de: "Alpha" }
    color_html { en: "HTML", zh: "HTML 值", tw: "HTML 值", ja: "HTML", ko: "HTML", fr: "HTML", de: "HTML" }
    ok { en: "OK", zh: "确定", tw: "確定", ja: "OK", ko: "확인", fr: "OK", de: "OK" }
    accent_custom { en: "Custom", zh: "自定义", tw: "自訂", ja: "カスタム", ko: "사용자 지정", fr: "Personnalisé", de: "Eigene" }
    ov_hint_copy { en: "Drag to select · Enter to copy · Esc to cancel", zh: "拖拽选区 · Enter 复制 · Esc 取消", tw: "拖曳選區 · Enter 複製 · Esc 取消", ja: "ドラッグで選択 · Enter でコピー · Esc で中止", ko: "드래그로 선택 · Enter 복사 · Esc 취소", fr: "Glissez pour sélectionner · Entrée copier · Échap annuler", de: "Ziehen zum Auswählen · Enter kopieren · Esc abbrechen" }
    ov_cancel_confirm { en: "Discard this capture?", zh: "放弃这次截图？", tw: "放棄這次截圖？", ja: "このキャプチャを破棄しますか？", ko: "이 캡처를 버릴까요?", fr: "Abandonner cette capture ?", de: "Diese Aufnahme verwerfen?" }
    ov_discard { en: "Discard", zh: "放弃", tw: "放棄", ja: "破棄", ko: "버리기", fr: "Abandonner", de: "Verwerfen" }
    ov_keep { en: "Keep editing", zh: "继续编辑", tw: "繼續編輯", ja: "編集を続ける", ko: "계속 편집", fr: "Continuer", de: "Weiter bearbeiten" }
    ov_hint_long { en: "Drag to select the scrolling area · Esc to cancel", zh: "拖拽选择滚动区域 · Esc 取消", tw: "拖曳選擇捲動區域 · Esc 取消", ja: "スクロール範囲をドラッグで選択 · Esc で中止", ko: "스크롤 영역을 드래그로 선택 · Esc 취소", fr: "Sélectionnez la zone défilante · Échap annuler", de: "Scrollbereich ziehen · Esc abbrechen" }
    ov_hint_record { en: "Drag to select the recording area · Esc to cancel", zh: "拖拽选择录制区域 · Esc 取消", tw: "拖曳選擇錄影區域 · Esc 取消", ja: "録画範囲をドラッグで選択 · Esc で中止", ko: "녹화 영역을 드래그로 선택 · Esc 취소", fr: "Sélectionnez la zone à enregistrer · Échap annuler", de: "Aufnahmebereich ziehen · Esc abbrechen" }
    tool_select { en: "Select", zh: "选择", tw: "選擇", ja: "選択", ko: "선택", fr: "Sélection", de: "Auswahl" }
    tool_rect { en: "Rect", zh: "矩形", tw: "矩形", ja: "矩形", ko: "사각형", fr: "Rectangle", de: "Rechteck" }
    tool_ellipse { en: "Ellipse", zh: "椭圆", tw: "橢圓", ja: "楕円", ko: "타원", fr: "Ellipse", de: "Ellipse" }
    tool_arrow { en: "Arrow", zh: "箭头", tw: "箭頭", ja: "矢印", ko: "화살표", fr: "Flèche", de: "Pfeil" }
    tool_line { en: "Line", zh: "直线", tw: "直線", ja: "直線", ko: "직선", fr: "Ligne", de: "Linie" }
    tool_pen { en: "Pen", zh: "画笔", tw: "畫筆", ja: "ペン", ko: "펜", fr: "Stylo", de: "Stift" }
    tool_highlight { en: "Highlight", zh: "高亮", tw: "高亮", ja: "ハイライト", ko: "강조", fr: "Surligner", de: "Markieren" }
    tool_mosaic { en: "Mosaic", zh: "马赛克", tw: "馬賽克", ja: "モザイク", ko: "모자이크", fr: "Mosaïque", de: "Mosaik" }
    tool_numbered { en: "Numbered", zh: "序号标注", tw: "序號標註", ja: "連番", ko: "번호 표시", fr: "Numérotation", de: "Nummerierung" }
    tool_text { en: "Text", zh: "文字", tw: "文字", ja: "テキスト", ko: "텍스트", fr: "Texte", de: "Text" }
    tool_measure { en: "Measure", zh: "标尺", tw: "標尺", ja: "測定", ko: "측정", fr: "Mesure", de: "Messung" }
    tool_lasso { en: "Freeform", zh: "自由选区", tw: "自由選區", ja: "自由選択", ko: "자유 선택", fr: "Forme libre", de: "Freihand" }
    ov_text_hint { en: "Type, Enter to place", zh: "输入文字，回车放置", tw: "輸入文字，Enter 放置", ja: "入力して Enter で確定", ko: "입력 후 Enter로 배치", fr: "Tapez, Entrée pour valider", de: "Tippen, Enter zum Bestätigen" }
    ov_paste { en: "Paste", zh: "粘贴", tw: "貼上", ja: "貼り付け", ko: "붙여넣기", fr: "Coller", de: "Einfügen" }
    ov_clear { en: "Clear", zh: "清空", tw: "清空", ja: "クリア", ko: "지우기", fr: "Effacer", de: "Leeren" }
    ov_undo { en: "Undo", zh: "撤销", tw: "還原", ja: "元に戻す", ko: "실행 취소", fr: "Annuler", de: "Rückgängig" }
    ov_redo { en: "Redo", zh: "重做", tw: "重做", ja: "やり直す", ko: "다시 실행", fr: "Rétablir", de: "Wiederholen" }
    ov_width { en: "Stroke width", zh: "线宽", tw: "線寬", ja: "線の太さ", ko: "선 두께", fr: "Épaisseur du trait", de: "Strichstärke" }
    // ---- image adjustments (editor "Adjustments" panel) ------------------
    f_adjust { en: "Adjustments", zh: "图像调整", tw: "圖像調整", ja: "調整", ko: "조정", fr: "Ajustements", de: "Anpassungen" }
    f_rotate90 { en: "Rotate 90° CW", zh: "顺时针 90°", tw: "順時針 90°", ja: "時計回り 90°", ko: "시계 방향 90°", fr: "Rotation 90° horaire", de: "90° im Uhrzeigersinn" }
    f_rotate270 { en: "Rotate 90° CCW", zh: "逆时针 90°", tw: "逆時針 90°", ja: "反時計回り 90°", ko: "반시계 90°", fr: "Rotation 90° anti-horaire", de: "90° gegen Uhrzeigersinn" }
    f_rotate180 { en: "Rotate 180°", zh: "旋转 180°", tw: "旋轉 180°", ja: "180° 回転", ko: "180° 회전", fr: "Rotation 180°", de: "180° drehen" }
    f_fliph { en: "Flip horizontal", zh: "水平翻转", tw: "水平翻轉", ja: "左右反転", ko: "좌우 대칭", fr: "Retourner H", de: "Horizontal spiegeln" }
    f_flipv { en: "Flip vertical", zh: "垂直翻转", tw: "垂直翻轉", ja: "上下反転", ko: "상하 대칭", fr: "Retourner V", de: "Vertikal spiegeln" }
    f_gray { en: "Grayscale", zh: "灰度", tw: "灰階", ja: "グレースケール", ko: "흑백", fr: "Niveaux de gris", de: "Graustufen" }
    f_invert { en: "Invert", zh: "反相", tw: "反相", ja: "反転", ko: "반전", fr: "Inverser", de: "Invertieren" }
    f_blur { en: "Blur", zh: "模糊", tw: "模糊", ja: "ぼかし", ko: "흐림", fr: "Flou", de: "Weichzeichnen" }
    f_emboss { en: "Emboss", zh: "浮雕", tw: "浮雕", ja: "エンボス", ko: "양각", fr: "Estampage", de: "Prägen" }
    f_close { en: "Close", zh: "关闭", tw: "關閉", ja: "閉じる", ko: "닫기", fr: "Fermer", de: "Schließen" }
    col_white { en: "White", zh: "白色", tw: "白色", ja: "白", ko: "흰색", fr: "Blanc", de: "Weiß" }
    col_red { en: "Red", zh: "红色", tw: "紅色", ja: "赤", ko: "빨강", fr: "Rouge", de: "Rot" }
    col_amber { en: "Amber", zh: "琥珀", tw: "琥珀", ja: "アンバー", ko: "호박색", fr: "Ambre", de: "Bernstein" }
    col_green { en: "Green", zh: "绿色", tw: "綠色", ja: "緑", ko: "초록", fr: "Vert", de: "Grün" }
    col_blue { en: "Blue", zh: "蓝色", tw: "藍色", ja: "青", ko: "파랑", fr: "Bleu", de: "Blau" }
    ov_copy { en: "Copy", zh: "复制", tw: "複製", ja: "コピー", ko: "복사", fr: "Copier", de: "Kopieren" }
    ov_pin { en: "Pin", zh: "贴图", tw: "貼圖", ja: "ピン", ko: "고정", fr: "Épingler", de: "Anheften" }
    ov_save { en: "Save", zh: "保存", tw: "儲存", ja: "保存", ko: "저장", fr: "Enregistrer", de: "Speichern" }
    ov_upload { en: "Upload", zh: "上传", tw: "上傳", ja: "アップロード", ko: "업로드", fr: "Envoyer", de: "Hochladen" }
    ov_cancel { en: "Cancel", zh: "取消", tw: "取消", ja: "中止", ko: "취소", fr: "Annuler", de: "Abbrechen" }
    ov_start_long { en: "Start scroll capture", zh: "开始长截图", tw: "開始長截圖", ja: "スクロール撮影を開始", ko: "스크롤 캡처 시작", fr: "Lancer la capture défilante", de: "Scroll-Aufnahme starten" }
    ov_start_long_manual { en: "Manual (I scroll)", zh: "手动长截图（自己滚）", tw: "手動長截圖（自己滾）", ja: "手動長截圖（自分でスクロール）", ko: "수동 스크롤 캡처(직접 스크롤)", fr: "Manuel (je défile)", de: "Manuell (ich scrolle)" }
    ov_start_record { en: "Start recording", zh: "开始录屏", tw: "開始錄影", ja: "録画を開始", ko: "녹화 시작", fr: "Démarrer l'enregistrement", de: "Aufzeichnung starten" }
    ov_ocr { en: "OCR", zh: "识别文字", tw: "辨識文字", ja: "文字認識", ko: "문자 인식", fr: "OCR", de: "OCR" }
    ov_ocr_running { en: "Recognising…", zh: "识别中…", tw: "辨識中…", ja: "認識中…", ko: "인식 중…", fr: "Reconnaissance…", de: "Erkenne…" }
    ov_ocr_title { en: "Recognised text", zh: "识别出的文字", tw: "辨識出的文字", ja: "認識した文字", ko: "인식된 텍스트", fr: "Texte reconnu", de: "Erkannter Text" }
    ov_ocr_words { en: "words", zh: "个词", tw: "個詞", ja: "語", ko: "단어", fr: "mots", de: "Wörter" }
    ov_ocr_copy { en: "Copy text", zh: "复制文字", tw: "複製文字", ja: "テキストをコピー", ko: "텍스트 복사", fr: "Copier le texte", de: "Text kopieren" }
    ov_ocr_close { en: "Close", zh: "关闭", tw: "關閉", ja: "閉じる", ko: "닫기", fr: "Fermer", de: "Schließen" }
    ov_ocr_empty { en: "No text was found in this region.", zh: "这片区域里没有识别到文字。", tw: "這片區域裡沒有辨識到文字。", ja: "この領域から文字が見つかりませんでした。", ko: "이 영역에서 텍스트를 찾지 못했습니다.", fr: "Aucun texte trouvé dans cette zone.", de: "In diesem Bereich wurde kein Text gefunden." }
    ov_ocr_hint { en: "Recognise text in the selection", zh: "识别选区内的文字", tw: "辨識選區內的文字", ja: "選択範囲の文字を認識", ko: "선택 영역의 텍스트 인식", fr: "Reconnaître le texte de la sélection", de: "Text in der Auswahl erkennen" }
    ov_qr_hint { en: "Scan the selection for QR codes", zh: "扫描选区内的二维码", tw: "掃描選區內的二維碼", ja: "選択範囲の QR コードを読み取る", ko: "선택 영역에서 QR 코드 스캔", fr: "Analyser la sélection pour des codes QR", de: "Auswahl nach QR-Codes durchsuchen" }
    ov_qr { en: "QR", zh: "二维码", tw: "二維碼", ja: "QR", ko: "QR", fr: "QR", de: "QR" }
    ov_qr_running { en: "Looking for QR codes…", zh: "正在查找二维码…", tw: "正在尋找二維碼…", ja: "QR コードを検索中…", ko: "QR 코드 검색 중…", fr: "Recherche de codes QR…", de: "Suche nach QR-Codes…" }
    ov_qr_title { en: "QR codes", zh: "二维码内容", tw: "二維碼內容", ja: "QR コード", ko: "QR 코드", fr: "Codes QR", de: "QR-Codes" }
    ov_qr_copy { en: "Copy", zh: "复制", tw: "複製", ja: "コピー", ko: "복사", fr: "Copier", de: "Kopieren" }
    ov_qr_close { en: "Close", zh: "关闭", tw: "關閉", ja: "閉じる", ko: "닫기", fr: "Fermer", de: "Schließen" }
    ov_qr_open { en: "Open link", zh: "打开链接", tw: "開啟連結", ja: "リンクを開く", ko: "링크 열기", fr: "Ouvrir le lien", de: "Link öffnen" }
    ov_qr_empty { en: "No QR code was found in this region.", zh: "这片区域里没有找到二维码。", tw: "這片區域裡沒有找到二維碼。", ja: "この領域に QR コードは見つかりませんでした。", ko: "이 영역에서 QR 코드를 찾지 못했습니다.", fr: "Aucun code QR trouvé dans cette zone.", de: "In diesem Bereich wurde kein QR-Code gefunden." }
    ov_qr_count { en: "code(s) found", zh: "个二维码", tw: "個二維碼", ja: "件のコード", ko: "개의 코드", fr: "code(s) trouvé(s)", de: "Code(s) gefunden" }
    ov_translate { en: "Translate", zh: "翻译", tw: "翻譯", ja: "翻訳", ko: "번역", fr: "Traduire", de: "Übersetzen" }
    ov_tr_running { en: "Translating…", zh: "翻译中…", tw: "翻譯中…", ja: "翻訳中…", ko: "번역 중…", fr: "Traduction…", de: "Übersetze…" }
    tr_settings { en: "Translation", zh: "翻译", tw: "翻譯", ja: "翻訳", ko: "번역", fr: "Traduction", de: "Übersetzung" }
    tr_provider { en: "Provider", zh: "服务商", tw: "服務商", ja: "プロバイダー", ko: "공급자", fr: "Fournisseur", de: "Anbieter" }
    tr_google { en: "Google (free)", zh: "Google（免费）", tw: "Google（免費）", ja: "Google（無料）", ko: "Google(무료)", fr: "Google (gratuit)", de: "Google (kostenlos)" }
    tr_deepl { en: "DeepL (API key)", zh: "DeepL（需密钥）", tw: "DeepL（需金鑰）", ja: "DeepL（API キー）", ko: "DeepL(API 키)", fr: "DeepL (clé API)", de: "DeepL (API-Schlüssel)" }
    tr_custom { en: "Custom endpoint", zh: "自定义服务", tw: "自訂服務", ja: "カスタム endpoint", ko: "사용자 지정", fr: "Endpoint personnalisé", de: "Eigener Endpunkt" }
    tr_endpoint { en: "Endpoint URL", zh: "服务地址", tw: "服務位址", ja: "エンドポイント URL", ko: "엔드포인트 URL", fr: "URL du service", de: "Endpunkt-URL" }
    tr_endpoint_hint { en: "LibreTranslate-compatible /translate URL", zh: "兼容 LibreTranslate 的 /translate 地址", tw: "相容 LibreTranslate 的 /translate 位址", ja: "LibreTranslate 互換の /translate URL", ko: "LibreTranslate 호환 /translate URL", fr: "URL /translate compatible LibreTranslate", de: "LibreTranslate-kompatible /translate-URL" }
    tr_target { en: "Target language", zh: "目标语言", tw: "目標語言", ja: "翻訳先の言語", ko: "대상 언어", fr: "Langue cible", de: "Zielsprache" }
    tr_key { en: "API key", zh: "密钥", tw: "金鑰", ja: "API キー", ko: "API 키", fr: "Clé API", de: "API-Schlüssel" }
    tr_hint { en: "Used by the Translate button on the OCR panel. Google needs access to international networks (use a proxy if blocked); DeepL needs a key (free keys end with “:fx”); Custom points at your own LibreTranslate-compatible server.", zh: "供识别面板的「翻译」按钮使用。Google 需能访问国际网络（被墙时请配代理）；DeepL 需要密钥（免费密钥以 “:fx” 结尾）；自定义可填自己的 LibreTranslate 兼容服务。", tw: "供辨識面板的「翻譯」按鈕使用。Google 需能存取國際網路（遭阻擋時請設代理）；DeepL 需要金鑰（免費金鑰以「:fx」結尾）；自訂可填自己的 LibreTranslate 相容服務。", ja: "OCR パネルの「翻訳」ボタンで使用します。Google は国際ネットワークへの接続が必要（遮断時はプロキシを設定）、DeepL はキーが必要（無料キーは「:fx」で終わる）、カスタムは自前の LibreTranslate 互換サーバーを指定します。", ko: "OCR 패널의 '번역' 버튼에 사용됩니다. Google은 국제 네트워크 접근이 필요하고(차단 시 프록시 설정), DeepL은 키가 필요하며(무료 키는 ':fx'로 끝남), 사용자 지정은 자체 LibreTranslate 호환 서버를 지정합니다.", fr: "Utilisé par le bouton Traduire du panneau OCR. Google nécessite l'accès au réseau international (configurez un proxy si bloqué) ; DeepL demande une clé (les clés gratuites finissent par « :fx ») ; Personnalisé pointe vers votre serveur compatible LibreTranslate.", de: "Wird vom Übersetzen-Button des OCR-Panels verwendet. Google braucht Zugang zum internationalen Netz (bei Blockade Proxy konfigurieren); DeepL braucht einen Schlüssel (kostenlose enden auf „:fx“); Eigener Endpunkt zeigt auf Ihren LibreTranslate-kompatiblen Server." }
    ov_tr_err_empty { en: "There is no text to translate.", zh: "没有可翻译的文字。", tw: "沒有可翻譯的文字。", ja: "翻訳するテキストがありません。", ko: "번역할 텍스트가 없습니다.", fr: "Aucun texte à traduire.", de: "Kein Text zum Übersetzen vorhanden." }
    ov_tr_err_key { en: "This provider needs an API key (Settings > Translation).", zh: "该服务需要密钥（设置 > 翻译）。", tw: "此服務需要金鑰（設定 > 翻譯）。", ja: "このサービスには API キーが必要です（設定 > 翻訳）。", ko: "이 서비스에는 API 키가 필요합니다(설정 > 번역).", fr: "Ce service nécessite une clé API (Paramètres > Traduction).", de: "Dieser Dienst benötigt einen API-Schlüssel (Einstellungen > Übersetzung)." }
    ov_tr_err_url { en: "Fill in the custom endpoint URL first (Settings > Translation).", zh: "请先填写自定义服务地址（设置 > 翻译）。", tw: "請先填寫自訂服務位址（設定 > 翻譯）。", ja: "先にカスタムエンドポイントの URL を入力してください（設定 > 翻訳）。", ko: "먼저 사용자 지정 엔드포인트 URL을 입력하세요(설정 > 번역).", fr: "Renseignez d'abord l'URL du service (Paramètres > Traduction).", de: "Bitte zuerst die Endpunkt-URL eintragen (Einstellungen > Übersetzung)." }
    ov_tr_err_network { en: "Cannot reach the translation service. Check the network/proxy, or switch to a custom endpoint.", zh: "无法连接翻译服务，请检查网络或代理；国内网络建议换用自定义服务。", tw: "無法連線翻譯服務，請檢查網路或代理；建議改用自訂服務。", ja: "翻訳サービスに接続できません。ネットワーク／プロキシを確認するか、カスタム endpoints に切り替えてください。", ko: "번역 서비스에 연결할 수 없습니다. 네트워크/프록시를 확인하거나 사용자 지정 엔드포인트로 전환하세요.", fr: "Service de traduction inaccessible. Vérifiez le réseau/proxy ou utilisez un endpoint personnalisé.", de: "Übersetzungsdienst nicht erreichbar. Netzwerk/Proxy prüfen oder eigenen Endpunkt verwenden." }
    ov_tr_err_response { en: "The translation service returned an unexpected response.", zh: "翻译服务返回了异常响应。", tw: "翻譯服務回傳了異常回應。", ja: "翻訳サービスが予期しない応答を返しました。", ko: "번역 서비스가 예상치 못한 응답을 반환했습니다.", fr: "Le service de traduction a renvoyé une réponse inattendue.", de: "Der Übersetzungsdienst hat eine unerwartete Antwort geliefert." }
    qr_gen_title { en: "QR code generator", zh: "二维码生成器", tw: "二維碼產生器", ja: "QR コード生成", ko: "QR 코드 생성기", fr: "Générateur de code QR", de: "QR-Code-Generator" }
    qr_gen_launch { en: "Open generator", zh: "打开生成器", tw: "開啟產生器", ja: "ジェネレーターを開く", ko: "생성기 열기", fr: "Ouvrir le générateur", de: "Generator öffnen" }
    qr_gen_hint { en: "Type text or a URL, then copy, save or pin the code.", zh: "输入文字或网址，然后复制、保存或贴图。", tw: "輸入文字或網址，然後複製、儲存或貼圖。", ja: "テキストや URL を入力し、コピー・保存・ピン留めできます。", ko: "텍스트나 URL을 입력한 뒤 복사·저장·고정할 수 있습니다.", fr: "Saisissez du texte ou une URL, puis copiez, enregistrez ou épinglez.", de: "Text oder URL eingeben, dann kopieren, speichern oder anheften." }
    qr_gen_input { en: "Text / URL", zh: "文字 / 网址", tw: "文字 / 網址", ja: "テキスト / URL", ko: "텍스트 / URL", fr: "Texte / URL", de: "Text / URL" }
    qr_gen_copy { en: "Copy image", zh: "复制图片", tw: "複製圖片", ja: "画像をコピー", ko: "이미지 복사", fr: "Copier l'image", de: "Bild kopieren" }
    qr_gen_error { en: "The text is too long for a QR code.", zh: "内容过长，无法生成二维码。", tw: "內容過長，無法產生二維碼。", ja: "テキストが長すぎて QR コードにできません。", ko: "텍스트가 너무 길어 QR 코드를 만들 수 없습니다.", fr: "Le texte est trop long pour un code QR.", de: "Der Text ist zu lang für einen QR-Code." }
    trim_title { en: "Trim recording", zh: "裁剪录屏", tw: "裁剪錄影", ja: "録画をトリミング", ko: "녹화 자르기", fr: "Rogner l'enregistrement", de: "Aufnahme zuschneiden" }
    trim_hint { en: "Cut the beginning and end. The original file is kept; the trimmed clip is saved beside it.", zh: "裁掉头尾。原文件保留，裁剪结果另存到同目录。", tw: "裁掉頭尾。原檔保留，裁剪結果另存到同目錄。", ja: "先頭と末尾をカットします。元ファイルは残り、結果は同じフォルダーに別名で保存されます。", ko: "앞뒤를 자릅니다. 원본은 유지되고 결과는 같은 폴더에 별도로 저장됩니다.", fr: "Coupez le début et la fin. L'original est conservé, le résultat est enregistré à côté.", de: "Anfang und Ende abschneiden. Das Original bleibt erhalten, das Ergebnis wird daneben gespeichert." }
    trim_start { en: "Start", zh: "起点", tw: "起點", ja: "開始", ko: "시작", fr: "Début", de: "Start" }
    trim_end { en: "End", zh: "终点", tw: "終點", ja: "終了", ko: "끝", fr: "Fin", de: "Ende" }
    trim_keep { en: "Keep", zh: "保留", tw: "保留", ja: "保持", ko: "유지", fr: "Conserver", de: "Behalten" }
    trim_apply { en: "Trim & save as", zh: "裁剪并另存", tw: "裁剪並另存", ja: "切り出して保存", ko: "잘라서 저장", fr: "Rogner et enregistrer", de: "Zuschneiden & speichern" }
    trim_skip { en: "Skip", zh: "跳过", tw: "跳過", ja: "スキップ", ko: "건너뛰기", fr: "Ignorer", de: "Überspringen" }
    trim_running { en: "Trimming…", zh: "正在裁剪…", tw: "正在裁剪…", ja: "切り出し中…", ko: "자르는 중…", fr: "Rognage…", de: "Zuschneiden…" }
    trim_done { en: "Trimmed clip saved", zh: "裁剪结果已保存", tw: "裁剪結果已儲存", ja: "切り出し結果を保存しました", ko: "잘라낸 클립을 저장했습니다", fr: "Clip rogné enregistré", de: "Zugeschnittener Clip gespeichert" }
    trim_failed { en: "Trim failed", zh: "裁剪失败", tw: "裁剪失敗", ja: "切り出しに失敗しました", ko: "자르기 실패", fr: "Échec du rognage", de: "Zuschneiden fehlgeschlagen" }
    trim_no_video { en: "No recording available to trim yet.", zh: "还没有可裁剪的录屏。", tw: "還沒有可裁剪的錄影。", ja: "切り出せる録画がまだありません。", ko: "자를 수 있는 녹화가 아직 없습니다.", fr: "Aucun enregistrement à rogner.", de: "Noch keine Aufnahme zum Zuschneiden vorhanden." }
    tray_trim_video { en: "Trim last recording", zh: "裁剪上一段录屏", tw: "裁剪上一段錄影", ja: "前回の録画をトリミング", ko: "마지막 녹화 자르기", fr: "Rogner le dernier enregistrement", de: "Letzte Aufnahme zuschneiden" }

    // ---- GIF editor ------------------------------------------------------
    nav_gif { en: "GIF", zh: "GIF", tw: "GIF", ja: "GIF", ko: "GIF", fr: "GIF", de: "GIF" }
    gif_title { en: "GIF editor", zh: "GIF 编辑", tw: "GIF 編輯", ja: "GIF 編集", ko: "GIF 편집", fr: "Éditeur GIF", de: "GIF-Editor" }
    gif_subtitle { en: "Trim, crop, add text, or convert to MP4.", zh: "裁剪片段、裁切画面、加文字，或转为 MP4。", tw: "剪裁片段、裁切畫面、加文字，或轉為 MP4。", ja: "切り取り・クロップ・文字追加、または MP4 変換。", ko: "자르기·크롭·텍스트 추가 또는 MP4 변환.", fr: "Découper, recadrer, ajouter du texte ou convertir en MP4.", de: "Zuschneiden, croppen, Text hinzufügen oder zu MP4." }
    gif_input { en: "Input GIF", zh: "输入 GIF", tw: "輸入 GIF", ja: "入力 GIF", ko: "입력 GIF", fr: "GIF source", de: "Eingabe-GIF" }
    gif_choose { en: "Choose…", zh: "选择…", tw: "選擇…", ja: "選択…", ko: "선택…", fr: "Choisir…", de: "Wählen…" }
    gif_operation { en: "Operation", zh: "操作", tw: "操作", ja: "操作", ko: "작업", fr: "Opération", de: "Vorgang" }
    gif_op_trim { en: "Trim (keep frames)", zh: "裁剪片段", tw: "裁剪片段", ja: "トリミング", ko: "자르기", fr: "Découper", de: "Zuschneiden" }
    gif_op_crop { en: "Crop (rectangle)", zh: "裁切画面", tw: "裁切畫面", ja: "クロップ", ko: "크롭", fr: "Recadrer", de: "Croppen" }
    gif_op_text { en: "Add text", zh: "添加文字", tw: "新增文字", ja: "文字追加", ko: "텍스트 추가", fr: "Texte", de: "Text" }
    gif_op_mp4 { en: "Convert to MP4", zh: "转为 MP4", tw: "轉為 MP4", ja: "MP4 変換", ko: "MP4 변환", fr: "Vers MP4", de: "Zu MP4" }
    gif_start_frame { en: "Start frame", zh: "起始帧", tw: "起始幀", ja: "開始フレーム", ko: "시작 프레임", fr: "Image de début", de: "Startframe" }
    gif_end_frame { en: "End frame (0 = last)", zh: "结束帧（0=末尾）", tw: "結束幀（0=末尾）", ja: "終了フレーム（0=最後）", ko: "끝 프레임(0=마지막)", fr: "Image de fin (0 = dernière)", de: "Endframe (0 = letztes)" }
    gif_x { en: "X", zh: "X", tw: "X", ja: "X", ko: "X", fr: "X", de: "X" }
    gif_y { en: "Y", zh: "Y", tw: "Y", ja: "Y", ko: "Y", fr: "Y", de: "Y" }
    gif_w { en: "Width", zh: "宽", tw: "寬", ja: "幅", ko: "너비", fr: "Largeur", de: "Breite" }
    gif_h { en: "Height", zh: "高", tw: "高", ja: "高さ", ko: "높이", fr: "Hauteur", de: "Höhe" }
    gif_text { en: "Text", zh: "文字内容", tw: "文字內容", ja: "テキスト", ko: "텍스트", fr: "Texte", de: "Text" }
    gif_font_size { en: "Font size", zh: "字号", tw: "字號", ja: "文字サイズ", ko: "글자 크기", fr: "Taille", de: "Schriftgröße" }
    gif_text_pos { en: "Position", zh: "位置", tw: "位置", ja: "位置", ko: "위치", fr: "Position", de: "Position" }
    gif_pos_tl { en: "Top-left", zh: "左上", tw: "左上", ja: "左上", ko: "왼쪽 위", fr: "Haut-gauche", de: "Oben links" }
    gif_pos_tr { en: "Top-right", zh: "右上", tw: "右上", ja: "右上", ko: "오른쪽 위", fr: "Haut-droite", de: "Oben rechts" }
    gif_pos_bl { en: "Bottom-left", zh: "左下", tw: "左下", ja: "左下", ko: "왼쪽 아래", fr: "Bas-gauche", de: "Unten links" }
    gif_pos_br { en: "Bottom-right", zh: "右下", tw: "右下", ja: "右下", ko: "오른쪽 아래", fr: "Bas-droite", de: "Unten rechts" }
    gif_pos_c { en: "Center", zh: "居中", tw: "居中", ja: "中央", ko: "가운데", fr: "Centre", de: "Mitte" }
    gif_speed { en: "Quality (1 fast – 30 best)", zh: "质量（1 快 – 30 好）", tw: "品質（1 快 – 30 好）", ja: "品質（1 速い – 30 良い）", ko: "품질(1 빠름 – 30 좋음)", fr: "Qualité (1 rapide – 30 meilleure)", de: "Qualität (1 schnell – 30 beste)" }
    gif_output { en: "Output", zh: "输出", tw: "輸出", ja: "出力", ko: "출력", fr: "Sortie", de: "Ausgabe" }
    gif_run { en: "Run", zh: "执行", tw: "執行", ja: "実行", ko: "실행", fr: "Lancer", de: "Ausführen" }
    gif_running { en: "Working…", zh: "处理中…", tw: "處理中…", ja: "処理中…", ko: "처리 중…", fr: "Traitement…", de: "Arbeitet…" }
    gif_done { en: "Saved", zh: "已保存", tw: "已儲存", ja: "保存しました", ko: "저장됨", fr: "Enregistré", de: "Gespeichert" }
    gif_failed { en: "Failed", zh: "失败", tw: "失敗", ja: "失敗", ko: "실패", fr: "Échec", de: "Fehler" }
    gif_need_input { en: "Choose an input GIF first.", zh: "请先选择输入 GIF。", tw: "請先選擇輸入 GIF。", ja: "まず入力 GIF を選択してください。", ko: "먼저 입력 GIF를 선택하세요.", fr: "Choisissez d'abord un GIF source.", de: "Wählen Sie zuerst ein Eingabe-GIF." }
    gif_open { en: "Open", zh: "打开", tw: "開啟", ja: "開く", ko: "열기", fr: "Ouvrir", de: "Öffnen" }
    gif_needs_ffmpeg { en: "MP4 conversion needs ffmpeg on PATH.", zh: "转 MP4 需要 PATH 中有 ffmpeg。", tw: "轉 MP4 需要 PATH 中有 ffmpeg。", ja: "MP4 変換には PATH の ffmpeg が必要です。", ko: "MP4 변환에는 PATH의 ffmpeg가 필요합니다.", fr: "La conversion MP4 nécessite ffmpeg dans le PATH.", de: "MP4 braucht ffmpeg im PATH." }
    tray_edit_gif { en: "Edit GIF", zh: "编辑 GIF", tw: "編輯 GIF", ja: "GIF 編集", ko: "GIF 편집", fr: "Éditer le GIF", de: "GIF bearbeiten" }
    tray_qr_gen { en: "QR generator", zh: "二维码生成", tw: "二維碼產生", ja: "QR 生成", ko: "QR 생성", fr: "Générateur QR", de: "QR-Generator" }
    tray_record_voice { en: "Record voice note", zh: "录制语音便签", tw: "錄製語音便籤", ja: "音声メモ録音", ko: "음성 메모 녹음", fr: "Mémo vocal", de: "Sprachnotiz" }
    voice_recording { en: "Recording voice note…", zh: "正在录制语音便签…", tw: "正在錄製語音便籤…", ja: "音声メモ録音中…", ko: "음성 메모 녹음 중…", fr: "Enregistrement vocal…", de: "Sprachnotiz aufnehmen…" }
    voice_done { en: "Voice note saved", zh: "语音便签已保存", tw: "語音便籤已儲存", ja: "音声メモを保存", ko: "음성 메모 저장됨", fr: "Mémo vocal enregistré", de: "Sprachnotiz gespeichert" }
    voice_failed { en: "Voice note failed", zh: "语音便签失败", tw: "語音便籤失敗", ja: "音声メモ失敗", ko: "음성 메모 실패", fr: "Échec du mémo vocal", de: "Sprachnotiz fehlgeschlagen" }
    // ---- automation (unattended captures + after-capture workflow) -------
    auto_done { en: "Automatic capture", zh: "自动截图完成", tw: "自動截圖完成", ja: "自動キャプチャ完了", ko: "자동 캡처 완료", fr: "Capture automatique", de: "Automatische Aufnahme" }
    auto_failed { en: "Automatic capture failed", zh: "自动截图失败", tw: "自動截圖失敗", ja: "自動キャプチャ失敗", ko: "자동 캡처 실패", fr: "Échec de la capture automatique", de: "Automatische Aufnahme fehlgeschlagen" }
    sec_automation { en: "Automation", zh: "自动化", tw: "自動化", ja: "自動化", ko: "자동화", fr: "Automatisation", de: "Automatisierung" }
    auto_hint { en: "Capture on a timer, at set times, or when a watched region changes. Runs in the background.", zh: "按定时、指定时刻或监控区域变化自动截图，在后台运行。", tw: "依定時、指定時刻或監控區域變化自動截圖，於背景執行。", ja: "タイマー・指定時刻・監視領域の変化で自動撮影します（バックグラウンド動作）。", ko: "타이머, 지정 시각, 감시 영역 변화에 따라 백그라운드에서 자동 캡처합니다.", fr: "Capture par minuterie, à heures fixes ou sur changement d'une zone surveillée (en arrière-plan).", de: "Automatisch per Timer, zu festen Zeiten oder bei Änderung eines überwachten Bereichs (im Hintergrund)." }
    auto_enable { en: "Enable automation", zh: "启用自动截图", tw: "啟用自動截圖", ja: "自動キャプチャを有効化", ko: "자동 캡처 사용", fr: "Activer l'automatisation", de: "Automatisierung aktivieren" }
    auto_interval { en: "Interval (seconds, 0 = off)", zh: "间隔（秒，0 为关闭）", tw: "間隔（秒，0 為關閉）", ja: "間隔（秒、0 で無効）", ko: "간격 (초, 0 = 끔)", fr: "Intervalle (secondes, 0 = désactivé)", de: "Intervall (Sekunden, 0 = aus)" }
    auto_daily { en: "Daily times (HH:MM, comma-separated)", zh: "每日时刻（HH:MM，逗号分隔）", tw: "每日時刻（HH:MM，逗號分隔）", ja: "毎日の時刻（HH:MM、カンマ区切り）", ko: "매일 시각 (HH:MM, 쉼표 구분)", fr: "Heures quotidiennes (HH:MM, séparées par des virgules)", de: "Tägliche Zeiten (HH:MM, durch Komma getrennt)" }
    auto_monitor { en: "Capture when a watched region changes", zh: "监控区域变化时截图", tw: "監控區域變化時截圖", ja: "監視領域の変化で撮影", ko: "감시 영역 변화 시 캡처", fr: "Capturer lors du changement d'une zone", de: "Bei Änderung des überwachten Bereichs aufnehmen" }
    auto_threshold { en: "Change sensitivity (lower = more sensitive)", zh: "变化阈值（越低越灵敏）", tw: "變化閾值（越低越靈敏）", ja: "変化しきい値（低いほど敏感）", ko: "변화 임계값 (낮을수록 민감)", fr: "Seuil de changement (bas = plus sensible)", de: "Änderungsschwelle (niedriger = empfindlicher)" }
    auto_workflow { en: "After-capture workflow", zh: "截图后工作流", tw: "截圖後工作流程", ja: "撮影後のワークフロー", ko: "캡처 후 워크플로", fr: "Workflow après capture", de: "Workflow nach der Aufnahme" }
    auto_workflow_hint { en: "Comma-separated: save, save:jpeg/webp/bmp/tiff/gif, copy-image, upload, copy-link, notify", zh: "逗号分隔：save、save:jpeg/webp/bmp/tiff/gif、copy-image、upload、copy-link、notify", tw: "逗號分隔：save、save:jpeg/webp/bmp/tiff/gif、copy-image、upload、copy-link、notify", ja: "カンマ区切り: save, save:jpeg/webp/bmp/tiff/gif, copy-image, upload, copy-link, notify", ko: "쉼표 구분: save, save:jpeg/webp/bmp/tiff/gif, copy-image, upload, copy-link, notify", fr: "Séparés par des virgules : save, save:jpeg/webp/bmp/tiff/gif, copy-image, upload, copy-link, notify", de: "Komma-getrennt: save, save:jpeg/webp/bmp/tiff/gif, copy-image, upload, copy-link, notify" }
    auto_max { en: "Max captures per session (0 = unlimited)", zh: "每次运行最多截图数（0 为不限）", tw: "每次執行最多截圖數（0 為不限）", ja: "セッションあたりの最大撮影数（0 = 無制限）", ko: "세션당 최대 캡처 수 (0 = 무제한)", fr: "Captures max par session (0 = illimité)", de: "Max. Aufnahmen pro Sitzung (0 = unbegrenzt)" }
    voice_needs_ffmpeg { en: "Voice recording needs ffmpeg on PATH.", zh: "录制语音需要 PATH 中有 ffmpeg。", tw: "錄製語音需要 PATH 中有 ffmpeg。", ja: "音声録音には PATH の ffmpeg が必要です。", ko: "음성 녹음에는 PATH의 ffmpeg가 필요합니다.", fr: "L'enregistrement vocal nécessite ffmpeg dans le PATH.", de: "Sprachaufnahme braucht ffmpeg im PATH." }
}

/// Convenience accessor: `t.tr.capture_title`.
pub struct Translator {
    pub lang: Lang,
    pub tr: &'static Strings,
}

impl Translator {
    pub fn new(lang: Lang) -> Self {
        Self {
            lang,
            tr: Strings::get(lang),
        }
    }

    pub fn set(&mut self, lang: Lang) {
        self.lang = lang;
        self.tr = Strings::get(lang);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_has_content() {
        for lang in Lang::ALL {
            let s = Strings::get(lang);
            assert!(!s.capture_title.is_empty());
            assert!(!s.act_area_title.is_empty());
            assert!(!s.settings_title.is_empty());
        }
    }

    #[test]
    fn locale_matching() {
        assert_eq!(Lang::from_locale("zh-CN"), Some(Lang::ZhHans));
        assert_eq!(Lang::from_locale("zh_TW"), Some(Lang::ZhHant));
        assert_eq!(Lang::from_locale("de-AT"), Some(Lang::De));
        assert_eq!(Lang::from_locale("ja_JP.UTF-8"), Some(Lang::Ja));
        assert_eq!(Lang::from_locale("xx"), None);
    }
}
