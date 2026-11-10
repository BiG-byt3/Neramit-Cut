//! Window layouts of Neramit Cut (main window and dialogs), built with ui::build.
//! Captions are English; i18n translates them (Thai is the default language).

use crate::ui::build::*;
use crate::ui::comp::Comp;
use crate::ui::theme;

// ------------------------------------------------------------------ main window

/// Height of the tinted header of a data card (toolbar 32 + margins 10/8 + 1 px outline)
/// and of its summary footer.
pub const CARD_HEAD: i32 = 51;
pub const CARD_FOOT: i32 = 34;

/// A data card: tinted header with the step badge and commands, the grid, a summary footer.
fn data_card(card_name: &str, tone: &str, tb_name: &str, buttons: Vec<N>, grid_name: &str) -> N {
    card(card_name)
        .p("Tone", tone)
        .p("HeadH", theme::px(CARD_HEAD))
        .p("FootH", theme::px(CARD_FOOT))
        .pad(1, 1, 1, CARD_FOOT)
        .kids(vec![toolbar(tb_name, buttons).top().h(32).m(10, 10, 10, 8), grid(grid_name).client().m(0, 0, 0, 0)])
}

pub fn main_window() -> Comp {
    // command bar under the blue caption: file and project commands, materials, settings, help
    let app_menu = toolbar(
        "tbAppMenu",
        vec![
            tool_drop("ab_file", "File", "document", true),
            tool_sep("as1"),
            tool("ab_open_project", "Open project", "open", true).hint("Open project (Ctrl+O)"),
            tool("ab_save_project", "Save project", "save", true).hint("Save project (Ctrl+S)"),
            tool_sep("as2"),
            tool("materials_mi", "Materials", "layers", true),
            tool_drop("ab_settings", "Settings", "settings", true),
            tool_drop("mi_runs", "Latest solutions", "history", true),
            tool_sep("as3"),
            tool_drop("ab_help", "Help", "help", true),
        ],
    )
    .client()
    .m(0, 10, 8, 10);
    let lang = toolbar("tbLang", vec![tool("lang_th", "TH", "", true), tool("lang_en", "EN", "", true)]).right().w(96).m(0, 10, 0, 10);
    let app_bar = panel("pAppBar").top().h(52).style("appbar").color(theme::SURFACE).pad(10, 0, 12, 0).kids(vec![app_menu, lang]);

    // every card: step badge and title, a few labelled commands, the rest under "More"
    let parts = data_card(
        "pParts",
        "blue",
        "parts_toolbar",
        vec![
            tool_title("parts_title", "Pieces").p("Step", 1),
            tool("parts_append_new_row_tb", "Add", "add", true).hint("Add a row"),
            tool("parts_delete_row_tb", "Delete", "delete", true).hint("Delete the selected row"),
            tool("parts_import_tb", "Import", "import", true).hint("Import from Excel / CSV"),
            tool_drop("parts_columns_tb", "Columns", "columns", true),
            tool_drop("parts_additional_commands_b", "More", "more", true),
        ],
        "sgParts",
    )
    .top()
    .h(380);
    let stock = data_card(
        "pStock",
        "blue",
        "tb_stock",
        vec![
            tool_title("stock_title", "Stock").p("Step", 2),
            tool("stock_append_new_row_tb", "Add", "add", true).hint("Add a row"),
            tool("stock_delete_row_tb", "Delete", "delete", true).hint("Delete the selected row"),
            tool("stock_import_tb", "Import", "import", true).hint("Import from Excel / CSV"),
            tool_drop("stock_columns_tb", "Columns", "columns", true),
            tool_drop("stock_additional_commands_b", "More", "more", true),
        ],
        "sgStock",
    )
    .client();
    // section 0: the name of the job (printed on the cutting plan)
    // at the top of the pieces and stock column, as wide as card 1; the plan card takes the full height
    let job = card("pJob").p("Tone", "blue").p("HeadH", theme::px(CARD_HEAD)).top().h(CARD_HEAD).pad(1, 1, 1, 1).m(0, 0, 0, 12).kids(vec![
        panel("pJobTitle").left().w(130).color(theme::BLUE.soft),
        panel("pJobDate").right().w(150).color(theme::BLUE.soft),
        edit("edJobName").client().m(0, 9, 6, 9).p("ShapedText", true).p("Cue", "Job name, e.g. Floor 1 slab, building A").font(&theme::font(theme::BODY)),
    ]);
    let left = panel("pBasicData").left().w(600).color(theme::BG).kids(vec![job, parts, splitter("spt_parts_stock", "alTop", 12), stock]);

    let results_tb = toolbar(
        "tb_results_menu",
        vec![
            tool_title("results_title", "Cutting plan").p("Step", 3),
            tool_primary("btnStart", "Calculate", "play").hint("Calculate (F9)"),
            tool("results_accept_undo_tb", "Accept", "check", true),
            tool_sep("rs1"),
            tool_drop("results_report_tb", "Report", "report", true).hint("The cutting plan report (PDF) and the other exports"),
            tool_drop("results_view_tb", "Display", "sliders", true),
        ],
    )
    .top()
    .h(32)
    .m(10, 10, 10, 8);
    let stat_sheet = |sheet_name: &str, caption: &str, grid_name: &str| sheet(sheet_name, caption).color(theme::SURFACE).kid(grid(grid_name).client().m(0, 0, 0, 0));
    let stats = pages("pcStatistics1D").client().p("Small", true).p("TabInset", theme::px(10)).color(theme::SURFACE).kids(vec![
        stat_sheet("tsUtilizedParts1D", "Pieces used", "pgUtilizedParts1D"),
        stat_sheet("tsUtilizedStock1D", "Bars used", "pgUtilizedStock1D"),
        stat_sheet("tsReusedWaste1D", "Offcuts", "pgReusedWaste1D"),
        stat_sheet("tsGarbageWaste1D", "Scrap", "pgGarbageWaste1D").visible(false),
        stat_sheet("tsMaterialStatistics1D", "By material", "pgMaterialStatistics1D"),
        stat_sheet("tsMiscel1D", "Summary", "pgMiscel1D"),
    ]);
    let results_pages = pages("pcResults").client().p("TabInset", theme::px(12)).color(theme::SURFACE).kids(vec![
        sheet("tsGraphic", "Cutting plan").color(theme::SURFACE).kid(grid("g_results_1d").client().m(0, 0, 0, 0)),
        sheet("tsStatistics", "Statistics").color(theme::SURFACE).kid(stats),
    ]);
    let right = card("pBasicResults").p("Tone", "blue").p("HeadH", theme::px(CARD_HEAD)).client().kids(vec![results_tb, results_pages]);
    let body = panel("pBody").client().color(theme::BG).pad(12, 12, 12, 12).kids(vec![left, splitter("pBasicSplitter", "alLeft", 12), right]);

    let menus = vec![
        popup(
            "pm_file",
            vec![
                item_icon("project_import_mi", "Open project…", "open").p("ShortCutText", "Ctrl+O"),
                item_icon("project_export_everything_mi", "Save project…", "save").p("ShortCutText", "Ctrl+S"),
                item_icon("project_export_selected_mi", "Export part of the project…", "export"),
                sep("fs1"),
                item_icon("project_about_mi", "Project notes…", "document"),
                sep("fs2"),
                item_icon("project_import_service_mi", "Restore a backup…", "undo"),
                item_icon("project_export_service_mi", "Back up everything…", "copy"),
                sep("fs3"),
                item_icon("miExit", "Exit", "close").p("ShortCutText", "Alt+F4"),
            ],
        ),
        popup(
            "pm_settings",
            vec![
                item_icon("settings_technical_1d_mi", "Cutting settings…", "settings"),
                item_icon("settings_technical_general_mi", "Prices and fractions…", "calculator"),
                item_icon("settings_interface_set_mi", "Appearance…", "sliders"),
                item_icon("mi_Language", "Language…", "globe"),
                item_icon("settings_working_dir_mi", "Working folder…", "folder"),
                sep("ss_1"),
                item("settings_technical_default_mi", "Reset cutting settings"),
                item("settings_interface_default_mi", "Reset appearance"),
            ],
        ),
        popup(
            "pm_help",
            vec![
                item_icon("miOnlineHelp", "Keyboard shortcuts", "help"),
                item_icon("mi_updates", "Check for updates…", "history"),
                sep("hs1"),
                item_icon("About1", "About Neramit Cut", "info"),
            ],
        ),
        popup(
            "parts_additional_commands_pm",
            vec![
                item_icon("parts_load_tb", "Open pieces list…", "open"),
                item_icon("parts_save_tb", "Save pieces list", "save"),
                item_icon("parts_save_as_mi", "Save as…", "save_as"),
                item_icon("parts_export_mi", "Export to Excel / CSV…", "export"),
                sep("pm0"),
                item_icon("parts_load_append_mi", "Open and append…", "open"),
                item_icon("parts_import_append_mi", "Import and append…", "import"),
                sep("pm2"),
                item_icon("parts_group_identical_mi", "Merge identical pieces", "list"),
                item_icon("parts_clear_mi", "Clear all pieces", "clear"),
            ],
        ),
        popup("parts_columns_pm", vec![item_check("parts_cols_index_mi", "Row number"), item_check("parts_cols_customer_mi", "Customer")]),
        popup(
            "stock_additional_commands_pm",
            vec![
                item_icon("stock_history_tb", "Stock history…", "history"),
                item_icon("stock_find_size_mi", "Find the best stock length…", "search"),
                sep("sm0"),
                item_icon("stock_export_mi", "Export to Excel / CSV…", "export"),
                item_icon("stock_import_append_mi", "Import and append…", "import"),
                sep("sm2"),
                item_icon("stock_group_identical_mi", "Merge identical bars", "list"),
                item_icon("stock_clear_grid_mi", "Clear all stock", "clear"),
            ],
        ),
        popup(
            "stock_columns_pm",
            vec![item_check("stock_cols_index_mi", "Row number"), item_check("stock_cols_price_mi", "Price"), item_check("stock_cols_weight_mi", "Weight per metre"), item_check("stock_cols_priority_mi", "Priority")],
        ),
        popup(
            "pm_report",
            vec![
                item_icon("results_report_pdf_mi", "Cutting plan report (PDF)…", "document"),
                sep("em0"),
                item_icon("results_save_as_csv_tb", "Table (Excel / CSV)…", "report"),
                item_icon("results_save_as_image_tb", "Image (PNG)…", "document"),
                item_icon("results_to_dxf_tb", "Drawing (DXF)…", "layers"),
                sep("em1"),
                item_icon("results_export_copy_mi", "Copy table", "copy"),
                item_icon("results_as_tabular_tb", "Piece table…", "table"),
            ],
        ),
        popup(
            "pm_additional_options_1d",
            vec![
                item_check("results_piece_length_show_1D_mi", "Piece sizes"),
                item_check("results_piece_label_show_1D_mi", "Piece labels"),
                item_check("results_waste_length_show_1D_mi", "Offcut sizes"),
                item_check("results_as_text_1D_mi", "Show as text"),
                sep("vm1"),
                item_check("results_pieces_GroupIdentical_1D_mi", "Group identical pieces"),
                item_check("results_piece_customer_show_1D_mi", "Customer names"),
                item_check("results_piece_angles_show_1D_mi", "Angles"),
                sep("vm2"),
                item_icon("results_columns_sub", "Table columns", "columns").kids(vec![
                    item_check("mi_show_pattern_column_results_1d", "Cutting formula (457+457+…)"),
                    sep("cm1"),
                    item_check("mi_show_material_column_results_1d", "Material"),
                    item_check("mi_show_waste_column_results_1d", "Offcut"),
                    item_check("mi_show_waste_percent_column_results_1d", "Offcut %"),
                    item_check("mi_show_price_column_results_1d", "Price"),
                    item_check("mi_show_mass_column_results_1d", "Mass"),
                    item_check("mi_show_area_column_results_1d", "Area"),
                    item_check("mi_show_image_column_results_1d", "Image"),
                ]),
            ],
        ),
        popup("results_1d_pm", vec![item_icon("results_1d_copy_mi", "Copy table", "copy")]),
    ];
    let mut f = N::new("TfMain", "f_main_window")
        .caption("Neramit Cut")
        .p("CaptionColor", theme::BRAND as i64)
        .p("CaptionTextColor", theme::WHITE as i64)
        .p("ClientWidth", theme::px(1360))
        .p("ClientHeight", theme::px(820))
        .p("Position", "poScreenCenter")
        .p("Constraints.MinWidth", theme::px(980))
        .p("Constraints.MinHeight", theme::px(620))
        .color(theme::BG)
        .font(&theme::font(theme::BODY))
        .kids(vec![app_bar, body]);
    for m in menus {
        f = f.kid(m);
    }
    f.0
}

// ------------------------------------------------------------------ dialogs

fn ok_cancel() -> Vec<N> {
    vec![modal_button("bbOk", "OK", 1, true), modal_button("bbCancel", "Cancel", 2, false)]
}

pub fn technical() -> Comp {
    let level = panel("r_level").top().h(70).kids(vec![
        panel("r_level_labels").top().h(22).kids(vec![muted("lShortestRunningTime", "Faster").left().w(200), muted("lLongestRunningTime", "More thorough").right().w(200).p("Alignment", "taRightJustify")]),
        trackbar("tbOptimizare", 1, 150).top().h(40),
    ]);
    dialog(
        "TfDialog",
        "f_technical_settings_1D_angles_window",
        "Cutting settings",
        600,
        vec![
            section("s_cut", "Saw and spacing", true),
            field_short("r_kerf", "lGrosimeaTaieturii", "Saw blade thickness (kerf)", edit("eCutterThickness1D"), 160, None),
            field_short("r_space", "l_space_between_pieces", "Space between pieces", edit("e_space_between_pieces"), 160, None),
            field_short("r_min", "lLatimeUtilizabila", "Shortest reusable offcut", edit("eLatimeMinimaUtilizabila1D"), 160, None),
            field_short("r_maxp", "lMaxPiecesPerBar", "Most pieces per bar", edit("eMaxPiecesPerBar"), 160, Some(("u_maxp", "0 = no limit"))),
            section("s_waste", "Allowed offcut", false),
            note("n_waste", "Leave 0 to accept any offcut length.", 1),
            field_short("r_wge", "lWasteGreaterThan", "Offcut at least", edit("eWasteGreaterThan"), 160, None),
            field_short("r_wle", "lWasteSmallerThan", "or at most", edit("eWasteSmallerThan"), 160, None),
            section("s_opt", "Optimization", false),
            radio_group("rgMinimize", "Goal", &["Least offcut", "Lowest price"], 0, 2).top().h(64),
            level,
            field_short("r_prec", "lPrecision", "Calculation precision", edit("ePrecision"), 160, None),
        ],
        ok_cancel(),
    )
    .0
}

pub fn general() -> Comp {
    dialog(
        "TfDialog",
        "f_technical_settings_general_window",
        "Prices and fractions",
        520,
        vec![
            section("s_frac", "Fractions", true),
            field("r_round", "lRoundAllFractionsTo", "Round fractions to", combo("cbRoundFractionsTo", &["1/2", "1/4", "1/8", "1/16", "1/32"], 3)),
            section("s_price", "Stock price", false),
            radio_group("rgInventoryPrice", "", &["Price per bar", "Price per unit of length"], 0, 1).top().h(80),
        ],
        ok_cancel(),
    )
    .0
}

pub fn interface() -> Comp {
    let sheet_fonts = sheet("ts_font", "Fonts").color(theme::SURFACE).pad(0, 16, 0, 0).kids(vec![
        field("r_font_draw", "l_font_drawings", "Cutting drawings", button("bb_graphic_font", "")),
        field("r_font_grid", "l_font_grids", "Tables", button("bb_grid_font", "")),
        note("n_fonts", "Noto Sans Thai is used for the program itself.", 1),
    ]);
    let sheet_numbers = sheet("tsOthers", "Numbers").color(theme::SURFACE).pad(0, 16, 0, 0).kids(vec![
        field_short("r_dsize", "l_digits_sizes", "Decimals for sizes", spin("se_digits_sizes", 0, 10), 120, None),
        field_short("r_dperc", "l_digits_percents", "Decimals for percentages", spin("se_digits_percent", 0, 10), 120, None),
        field_short("r_dprice", "l_digits_price", "Decimals for prices", spin("se_digits_price", 0, 10), 120, None),
        check("cbGroupPartsAfterLoadingFromFile", "Merge identical pieces when opening files", false).top().m(0, 8, 0, 0),
    ]);
    let sheet_company = sheet("tsCompanyInfo", "Company").color(theme::SURFACE).pad(0, 12, 0, 0).kids(vec![
        field("r_cname", "lName", "Company name", edit("eCompanyName")),
        field("r_addr1", "lAddress", "Address", edit("eAddress1")),
        field("r_addr2", "lAddress2", "", edit("eAddress2")),
        field("r_city", "lCity", "City", edit("eCity")),
        field("r_zip", "lZip", "Postal code", edit("eZip")),
        field("r_email", "lEmail", "Email", edit("eEmail")),
        field("r_phone", "lPhone", "Phone", edit("ePhone")),
        field_short("r_logo", "lLogo", "Logo", button("bBrowse", "Browse…"), 120, Some(("lLogoFile", ""))),
    ]);
    dialog(
        "TfDialog",
        "f_interface_settings_window",
        "Appearance",
        640,
        vec![pages("pc_settings").top().h(42 + 12 + 8 * 40 + 8).p("TabInset", 0).color(theme::SURFACE).kids(vec![sheet_fonts, sheet_numbers, sheet_company])],
        ok_cancel(),
    )
    .0
}

pub fn language() -> Comp {
    dialog(
        "TfDialog",
        "f_choose_lang_window",
        "Language",
        440,
        vec![section("s_lang", "Language of the program", true), radio_group("rgLanguage", "", &["ไทย", "English"], 0, 1).top().h(76), note("n_lang", "The change applies at once.", 1)],
        ok_cancel(),
    )
    .0
}

pub fn materials() -> Comp {
    let tb = toolbar(
        "ToolBar1",
        vec![tool("tb_append_row", "Add", "add", true), tool("tb_delete", "Delete row", "delete", false), tool_sep("ms1"), tool("tb_print", "Print", "print", false), tool("tb_export", "Export to Excel / CSV", "export", false)],
    )
    .top()
    .h(32)
    .m(0, 0, 0, 10);
    let grid_card = card("pGridCard").top().h(340).pad(1, 1, 1, 8).kid(grid("pg_data").client());
    dialog("TfDialog", "f_MaterialsTypes1D_window", "Materials", 860, vec![tb, grid_card], ok_cancel()).0
}

pub fn project_about() -> Comp {
    dialog(
        "TfDialog",
        "f_project_about_window",
        "Project notes",
        560,
        vec![muted("l_project_name", "").top().p("AutoSize", false).h(22), memo("m_comment").top().h(220).m(0, 4, 0, 0)],
        ok_cancel(),
    )
    .0
}

pub fn project_options() -> Comp {
    dialog(
        "TfDialog",
        "f_project_import_export_options_window",
        "Project contents",
        560,
        vec![
            section("s_inc", "Include", true),
            check("cb_parts", "Pieces", true).top(),
            check("cb_stock", "Stock", true).top(),
            check("cb_settings", "Cutting settings", true).top(),
            check("cb_materials", "Materials", true).top(),
            check("cb_results", "Results", true).top(),
            section("s_comment", "Comment", false),
            memo("m_comment").top().h(120),
        ],
        ok_cancel(),
    )
    .0
}

pub fn about() -> Comp {
    let head = panel("pAboutHead").top().h(96).color(theme::SURFACE);
    let buttons_row = panel("r_about_buttons").top().h(44).m(0, 12, 0, 0).kids(vec![button("b_license_agreement", "License").left().w(150).m(0, 6, 8, 6), button("b_libraries", "Open-source components").left().w(220).m(0, 6, 8, 6), button("b_updates", "Check for updates").left().w(150).m(0, 6, 0, 6)]);
    dialog(
        "TfDialog",
        "f_about_window",
        "About Neramit Cut",
        620,
        vec![
            head,
            field("r_lic", "lLicensedTo_label", "License", label("lLicenseeName", "Free to use").p("Layout", "tlCenter").p("AutoSize", false)),
            field("r_dev", "l_manufacturer_string", "Developer", label("l_manufacturer_name", "Neramit Cut made by BiGByte Studio").p("Layout", "tlCenter").p("AutoSize", false)),
            field("r_font", "l_font_string", "Typeface", label("l_font_name", "Noto Sans Thai · SIL OFL 1.1").p("Layout", "tlCenter").p("AutoSize", false)),
            buttons_row,
        ],
        vec![modal_button("bb_ok", "Close", 1, true)],
    )
    .0
}

pub fn history() -> Comp {
    let list = card("pHistoryList").left().w(300).pad(1, 1, 1, 8).kid(grid("sgInventoryHistory").client());
    let preview = card("pHistoryWidth").client().pad(1, 1, 1, 8).kids(vec![title("pPreview", "Preview", 14).top().p("AutoSize", false).h(40).m(14, 6, 0, 0).p("Layout", "tlCenter"), grid("pgPreviewHistory").client()]);
    let row = panel("r_history").top().h(380).kids(vec![list, panel("r_hgap").left().w(12), preview]);
    dialog(
        "TfDialog",
        "fInventoryHistory",
        "Stock history",
        980,
        vec![note("n_hist", "A copy of the stock is kept each time a solution is accepted.", 1), row],
        vec![modal_button("bbRestore", "Restore this stock", 100, true), modal_button("bbCancel", "Cancel", 2, false)],
    )
    .0
}

pub fn find_size() -> Comp {
    let row1 = panel("r_find1").top().h(64).kids(vec![
        panel("c_mat").left().w(260).m(0, 0, 12, 0).kids(vec![muted("lMaterialName", "Material").top().h(20), N::new("TComboBox", "cbMaterialName").p("Style", "csDropDownList").top().h(32)]),
        panel("c_h").left().w(180).kids(vec![muted("lWidth", "Height").top().h(20), N::new("TComboBox", "cbWidth").p("Style", "csDropDownList").top().h(32)]),
    ]);
    let row2 = panel("r_find2").top().h(64).m(0, 6, 0, 0).kids(vec![
        panel("c_min").left().w(150).m(0, 0, 12, 0).kids(vec![muted("lMinRange", "Shortest").top().h(20), edit("eMinLengthRange").top()]),
        panel("c_max").left().w(150).m(0, 0, 12, 0).kids(vec![muted("lMaxRange", "Longest").top().h(20), edit("eMaxLengthRange").top()]),
        panel("c_step").left().w(120).m(0, 0, 16, 0).kids(vec![muted("lStepRange", "Step").top().h(20), edit("eStepLengthRange").top()]),
        primary("bbStartFindOptimalLength", "Search").icon("search").left().w(120).m(0, 20, 8, 12),
        button("bbStopFindOptimalLength", "Stop").icon("stop").left().w(100).m(0, 20, 0, 12),
    ]);
    dialog(
        "TfDialog",
        "f_FindOptimalSizeOfStock_window",
        "Find the best stock length",
        820,
        vec![
            note("n_find", "Tries every stock length in the range and ranks them by how well the pieces fit.", 1),
            row1,
            row2,
            progress("pbOptimalLength").top().m(0, 10, 0, 10),
            card("pFindGrid").top().h(280).pad(1, 1, 1, 8).kid(grid("pgOptimalLength").client()),
        ],
        vec![modal_button("bbAdd", "Add to stock", 1, true), modal_button("bbCancel", "Cancel", 2, false)],
    )
    .0
}

pub fn import_data() -> Comp {
    dialog(
        "TfDialog",
        "f_import_data_window",
        "Import data",
        900,
        vec![
            note("l_preview", "Check the columns. Click a column header to choose which column of the file it reads. Untick the rows you do not want.", 1),
            check("cbHeader", "The first row is a header (column names)", true).top().h(30),
            card("pImportGrid").top().h(380).pad(1, 1, 1, 8).kid(grid("pgData").client()),
        ],
        vec![modal_button("bbOk", "Import", 1, true), modal_button("bbCancel", "Cancel", 2, false)],
    )
    .0
}

pub fn tabular() -> Comp {
    let tb = toolbar("ToolBar1", vec![tool("tb_export", "Export to Excel / CSV", "export", true), tool("tb_print", "Print", "print", true), tool_drop("columns_tb", "Columns", "columns", true)]).top().h(32).m(0, 0, 0, 10);
    let menu = popup(
        "tabular_1d_columns_pm",
        vec![
            item("mi_show_index_column_results_1d", "Row number"),
            item("mi_show_width_column_results_1d", "Height"),
            item("mi_show_angles_column_results_1d", "Angles"),
            item("mi_show_position_top_column_results_1d", "Positions (top)"),
            item("mi_show_position_bottom_column_results_1d", "Positions (bottom)"),
            item("mi_show_label_column_results_1d", "Label"),
            item("mi_show_customer_column_results_1d", "Customer"),
        ],
    );
    let f = dialog("TfDialog", "f_tabular_view_window", "Pieces of the bar", 900, vec![tb, card("pTabGrid").top().h(400).pad(1, 1, 1, 8).kid(grid("g_pieces_1d").client())], vec![modal_button("bb_ok", "Close", 1, true)]);
    f.kid(menu).0
}

pub fn labels(stock: bool) -> Comp {
    let mut what = vec![
        section("s_what", "Print on each label", true),
        check("cbPieceSize", "Length", true).top(),
        check("cbCompanyName", "Company name", true).top(),
        check(if stock { "cbInventoryLabel" } else { "cbDemandLabel" }, "Label", true).top(),
        check(if stock { "cbMaterial" } else { "cbMaterialType" }, "Material", true).top(),
    ];
    if !stock {
        what.push(check("cbClientName", "Customer", true).top());
        what.push(check("cbDate", "Today's date", true).top());
    }
    what.push(check("cbCustomMessage", "Custom text", true).top());
    what.push(edit("eCustomMessage").top().m(28, 2, 0, 0));
    let what_h: i32 = what.iter().map(|n| n.outer_h()).sum();
    let left = panel("pLabelOptions").left().w(370).kids(vec![
        section("s_layout", "Sheet layout", true),
        field_short("r_rows", "lRows", "Rows per sheet", spin("eRows", 1, 20), 90, None),
        field_short("r_cols", "lColumns", "Columns per sheet", spin("eColumns", 1, 5), 90, None),
        field_short("r_font", "lFont", "Font", button("bbFont", ""), 190, None),
        section("s_margins", "Paper margins (%)", false),
        panel("r_margins1").top().h(40).kids(vec![
            muted("lml", "Left").left().w(60).p("Layout", "tlCenter").p("AutoSize", false),
            edit("eLeftStickerMargin").left().w(70).m(0, 4, 16, 4),
            muted("lmr", "Right").left().w(60).p("Layout", "tlCenter").p("AutoSize", false),
            edit("eRightStickerMargin").left().w(70).m(0, 4, 0, 4),
        ]),
        panel("r_margins2").top().h(40).kids(vec![
            muted("lmt", "Top").left().w(60).p("Layout", "tlCenter").p("AutoSize", false),
            edit("eTopStickerMargin").left().w(70).m(0, 4, 16, 4),
            muted("lmb", "Bottom").left().w(60).p("Layout", "tlCenter").p("AutoSize", false),
            edit("eBottomStickerMargin").left().w(70).m(0, 4, 0, 4),
        ]),
        field_short("r_inner", "lMarginsOfTheLabel", "Inside each label (%)", edit("eMarginsOfTheLabel"), 70, None),
    ]);
    let right = panel("pLabelRight").client().pad(24, 0, 0, 0).kids(vec![panel("pWhat").top().p("Height", what_h).kids(what), panel("pLabelPreview").top().h(150).m(0, 14, 0, 0).color(theme::SURFACE), muted("lNr1D", "").top().p("AutoSize", false).h(22).m(0, 8, 0, 0)]);
    let content_h = (what_h + theme::px(150 + 14 + 30)).max(theme::px(26 + 3 * 40 + 18 + 26 + 4 + 3 * 40));
    let row = panel("r_labels").top().p("Height", content_h).kids(vec![left, right]);
    dialog(
        "TfDialog",
        if stock { "f_adhesive_labels_stock_settings_window" } else { "f_adhesive_labels_parts_settings_window" },
        if stock { "Stock stickers" } else { "Piece stickers" },
        860,
        vec![row],
        vec![modal_button("btnPrint", "Print", 1, true).icon("print"), modal_button("bbCancel", "Cancel", 2, false)],
    )
    .0
}

pub fn dxf() -> Comp {
    let left = panel("c_dxf_left").left().w(280).kids(vec![
        section("s_bars", "Bars", true),
        check("cb_export_bar_contour", "Outline", true).top(),
        check("cb_export_info_bar", "Bar information", true).top(),
        section("s_pieces", "Pieces", false),
        check("cb_export_parts_contour", "Outline", true).top(),
        check("cb_export_parts_size", "Sizes", true).top(),
        check("cb_export_parts_label", "Labels", true).top(),
        check("cb_export_customer_name", "Customer", false).top(),
        check("cb_export_angles", "Angles", false).top(),
        section("s_offcuts", "Offcuts", false),
        check("cb_export_waste_contour", "Outline", true).top(),
        check("cb_export_waste_size", "Sizes", true).top(),
    ]);
    let narrow = |row: &str, l: &str, cap: &str, e: N| {
        let lab = label(l, cap).left().w(120).p("Layout", "tlCenter").p("AutoSize", false);
        panel(row).top().h(ROW_H).kids(vec![lab, e.client().m(0, 4, 0, 4)])
    };
    let right = panel("c_dxf_right").client().pad(24, 0, 0, 0).kids(vec![
        section("s_units", "Drawing", true),
        narrow("r_unit", "l_measurement_unit", "Unit", combo("cb_measurement_unit", &["Unitless", "Inches", "Feet", "Miles", "Millimeters", "Centimeters", "Meters", "Kilometers"], 4)),
        narrow("r_fh", "l_font_height", "Text height", edit("e_font_height")),
        section("s_layers", "Layer names", false),
        narrow("r_lt", "l_text", "Text", edit("e_Text")),
        narrow("r_lb", "l_bars", "Bars", edit("e_sheet")),
        narrow("r_lp", "l_parts", "Pieces", edit("e_parts")),
        narrow("r_lw", "l_waste", "Offcuts", edit("e_waste")),
    ]);
    let h = 26 + 2 * 28 + 48 + 5 * 28 + 48 + 2 * 28;
    dialog("TfDialog", "f_export_dxf_options_1d_window", "Export DXF", 680, vec![panel("r_dxf").top().h(h).kids(vec![left, right])], vec![modal_button("bbOK", "Export", 1, true), modal_button("bb_cancel", "Cancel", 2, false)]).0
}

pub fn print_stats() -> Comp {
    dialog(
        "TfDialog",
        "fPrintStatisticsOptions1D",
        "Print statistics",
        460,
        vec![
            section("s_ps", "Tables to print", true),
            check("cbUtilizedParts1D", "Pieces used", true).top(),
            check("cbUtilizedStock1D", "Bars used", true).top(),
            check("cbReusedWaste1D", "Offcuts", true).top(),
            check("cbGarbageWaste1D", "Scrap", false).top().visible(false),
            check("cbMaterialStatistics1D", "By material", true).top(),
            check("cbMiscel1D", "Summary", true).top(),
        ],
        vec![modal_button("bbOk", "Print", 1, true).icon("print"), modal_button("bbCancel", "Cancel", 2, false)],
    )
    .0
}

pub fn progress_window() -> Comp {
    dialog(
        "TfDialog",
        "f_optimization_progress_window",
        "Calculating",
        440,
        vec![title("lProgressTitle", "Calculating the cutting plan…", 15).top().p("AutoSize", false).h(28), progress("pb_optimization_progress").top().m(0, 10, 0, 10), muted("lProgressInfo", "").top().p("AutoSize", false).h(20)],
        vec![modal_button("bbStop", "Stop", 2, false).icon("stop")],
    )
    .0
}
