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
    history_exported { en: "images exported to PDF", zh: "张图片已导出为 PDF", tw: "張圖片已匯出為 PDF", ja: "枚を PDF に書き出し", ko: "개 이미지 PDF 내보냄", fr: "images exportées en PDF", de: "Bilder als PDF exportiert" }
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
    rec_audio { en: "Record audio", zh: "录制声音", tw: "錄製聲音", ja: "音声を録画", ko: "오디오 녹음", fr: "Enregistrer l'audio", de: "Audio aufnehmen" }
    rec_audio_device { en: "Audio device", zh: "音频设备", tw: "音訊裝置", ja: "オーディオデバイス", ko: "오디오 장치", fr: "Périphérique audio", de: "Audiogerät" }
    rec_audio_hint { en: "Needs ffmpeg. Blank = default device (Windows auto-detects).", zh: "需要 ffmpeg。留空用默认设备（Windows 自动检测）。", tw: "需要 ffmpeg。留空用預設裝置（Windows 自動偵測）。", ja: "ffmpeg が必要。空欄は既定デバイス（Windows 自動検出）。", ko: "ffmpeg 필요. 비우면 기본 장치(Windows 자동 감지).", fr: "Nécessite ffmpeg. Vide = périphérique par défaut (détection auto Windows).", de: "Benötigt ffmpeg. Leer = Standardgerät (Windows automatisch)." }
    rec_unlimited { en: "Unlimited", zh: "不限", tw: "不限", ja: "無制限", ko: "제한 없음", fr: "Illimité", de: "Unbegrenzt" }
    rec_hud_recording { en: "Recording", zh: "录制中", tw: "錄製中", ja: "録画中", ko: "녹화 중", fr: "Enregistrement", de: "Aufnahme" }
    rec_hud_paused { en: "Paused", zh: "已暂停", tw: "已暫停", ja: "一時停止", ko: "일시 정지", fr: "En pause", de: "Pausiert" }
    rec_hud_pause { en: "Pause", zh: "暂停", tw: "暫停", ja: "一時停止", ko: "일시 정지", fr: "Pause", de: "Pause" }
    rec_hud_resume { en: "Resume", zh: "继续", tw: "繼續", ja: "再開", ko: "재개", fr: "Reprendre", de: "Fortsetzen" }
    rec_hud_stop { en: "Stop", zh: "停止", tw: "停止", ja: "停止", ko: "정지", fr: "Arrêter", de: "Stopp" }
    rec_hud_hint { en: "Esc stops the recording", zh: "按 Esc 停止录制", tw: "按 Esc 停止錄製", ja: "Esc で録画を停止", ko: "Esc로 녹화 정지", fr: "Échap arrête l'enregistrement", de: "Esc beendet die Aufnahme" }
    rec_annotate { en: "Annotate", zh: "标注", tw: "標註", ja: "注釈", ko: "주석", fr: "Annoter", de: "Anmerken" }
    mk_arrow { en: "Arrow", zh: "箭头", tw: "箭頭", ja: "矢印", ko: "화살표", fr: "Flèche", de: "Pfeil" }
    mk_line { en: "Line", zh: "直线", tw: "直線", ja: "直線", ko: "직선", fr: "Ligne", de: "Linie" }
    mk_rect { en: "Rectangle", zh: "矩形", tw: "矩形", ja: "矩形", ko: "사각형", fr: "Rectangle", de: "Rechteck" }
    mk_ellipse { en: "Ellipse", zh: "椭圆", tw: "橢圓", ja: "楕円", ko: "타원", fr: "Ellipse", de: "Ellipse" }
    mk_text { en: "Text", zh: "文字", tw: "文字", ja: "テキスト", ko: "텍스트", fr: "Texte", de: "Text" }
    mk_emoji { en: "Emoji", zh: "表情", tw: "表情", ja: "絵文字", ko: "이모지", fr: "Emoji", de: "Emoji" }
    mk_clear { en: "Clear", zh: "清除", tw: "清除", ja: "クリア", ko: "지우기", fr: "Effacer", de: "Löschen" }
    mk_close { en: "Close", zh: "关闭", tw: "關閉", ja: "閉じる", ko: "닫기", fr: "Fermer", de: "Schließen" }
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
    upd_note { en: "PinSight has no update server of its own; this opens the releases page.", zh: "PinSight 没有自建更新服务器，这里会打开 Releases 页面。", tw: "PinSight 沒有自建更新伺服器，這裡會開啟 Releases 頁面。", ja: "独自の更新サーバーはありません。Releases ページを開きます。", ko: "자체 업데이트 서버가 없어 Releases 페이지를 엽니다.", fr: "Pas de serveur de mise à jour : ouvre la page des versions.", de: "Kein eigener Update-Server; öffnet die Releases-Seite." }
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
    out_decorations_hint { en: "Applied to every saved capture before it is written.", zh: "保存前自动应用到每张截图。", tw: "儲存前自動套用到每張截圖。", ja: "保存前にすべてのキャプチャへ自動適用します。", ko: "저장 전 모든 캡처에 자동 적용됩니다.", fr: "Appliqué à chaque capture avant l'enregistrement.", de: "Wird vor dem Speichern auf jede Aufnahme angewendet." }
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
    upd_proxy_note { en: "PinSight opens the releases page directly (no auto-download yet); this proxy is stored for a future updater.", zh: "PinSight 直接打开 Releases 页面（暂不支持自动下载），此代理为将来的更新器预留。", tw: "PinSight 直接開啟 Releases 頁面（尚不支援自動下載），此代理為未來的更新器預留。", ja: "PinSight は Releases ページを直接開きます（自動ダウンロードなし）。このプロキシは将来の更新機能のためです。", ko: "PinSight은 릴리스 페이지를 직접 엽니다(자동 다운로드 없음). 이 프록시는 향후 업데이터를 위한 것입니다.", fr: "PinSight ouvre la page des versions (pas de téléchargement auto) ; ce proxy est réservé à un futur updater.", de: "PinSight öffnet die Releases-Seite direkt (kein Auto-Download); dieser Proxy ist für einen künftigen Updater." }

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
    ov_text_hint { en: "Type, Enter to place", zh: "输入文字，回车放置", tw: "輸入文字，Enter 放置", ja: "入力して Enter で確定", ko: "입력 후 Enter로 배치", fr: "Tapez, Entrée pour valider", de: "Tippen, Enter zum Bestätigen" }
    ov_clear { en: "Clear", zh: "清空", tw: "清空", ja: "クリア", ko: "지우기", fr: "Effacer", de: "Leeren" }
    ov_undo { en: "Undo", zh: "撤销", tw: "還原", ja: "元に戻す", ko: "실행 취소", fr: "Annuler", de: "Rückgängig" }
    ov_redo { en: "Redo", zh: "重做", tw: "重做", ja: "やり直す", ko: "다시 실행", fr: "Rétablir", de: "Wiederholen" }
    ov_width { en: "Stroke width", zh: "线宽", tw: "線寬", ja: "線の太さ", ko: "선 두께", fr: "Épaisseur du trait", de: "Strichstärke" }
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
    ov_ocr_unavailable { en: "OCR uses the OS engine: Windows.Media.Ocr, macOS Vision, or Tesseract on Linux.", zh: "文字识别使用系统引擎：Windows.Media.Ocr、macOS Vision，或 Linux 上的 Tesseract。", tw: "文字辨識使用系統引擎：Windows.Media.Ocr、macOS Vision，或 Linux 上的 Tesseract。", ja: "OCR は OS のエンジンを使用します（Windows.Media.Ocr / macOS Vision / Linux の Tesseract）。", ko: "OCR은 OS 엔진을 사용합니다(Windows.Media.Ocr, macOS Vision, Linux의 Tesseract).", fr: "L'OCR utilise le moteur du système : Windows.Media.Ocr, Vision sur macOS, ou Tesseract sur Linux.", de: "OCR nutzt die Engine des Systems: Windows.Media.Ocr, Vision unter macOS oder Tesseract unter Linux." }

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
    tray_record_voice { en: "Record voice note", zh: "录制语音便签", tw: "錄製語音便籤", ja: "音声メモ録音", ko: "음성 메모 녹음", fr: "Mémo vocal", de: "Sprachnotiz" }
    voice_recording { en: "Recording voice note…", zh: "正在录制语音便签…", tw: "正在錄製語音便籤…", ja: "音声メモ録音中…", ko: "음성 메모 녹음 중…", fr: "Enregistrement vocal…", de: "Sprachnotiz aufnehmen…" }
    voice_done { en: "Voice note saved", zh: "语音便签已保存", tw: "語音便籤已儲存", ja: "音声メモを保存", ko: "음성 메모 저장됨", fr: "Mémo vocal enregistré", de: "Sprachnotiz gespeichert" }
    voice_failed { en: "Voice note failed", zh: "语音便签失败", tw: "語音便籤失敗", ja: "音声メモ失敗", ko: "음성 메모 실패", fr: "Échec du mémo vocal", de: "Sprachnotiz fehlgeschlagen" }
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
