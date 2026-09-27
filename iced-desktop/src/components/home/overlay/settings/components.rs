use macros::section;

section!(
    GeneralSection,
    [
        section(
            "Language/Region",
            [
                hour_format: deplace_core::settings::HourFormat,
                date_format: deplace_core::settings::DateFormat,
                spacer(),
                first_day_of_week: deplace_core::settings::DayOfWeek,
                timezone: chrono_tz::Tz,
            ]
        ),
        section("Units", [data_size_unit: deplace_core::settings::DataSizeUnit]),
        section("Behavior", [minimize_to_tray: bool]),
        section(
            "Profile customization",
            [
                play_user_theme_on_click: bool,
                use_banner_colors: bool,
                use_name_color: bool,
            ]
        ),
    ]
);

section!(
    AppearanceSection,
    [name_decoration: deplace_core::settings::NameDecoration]
);

section!(AudioSection, []);

section!(
    ChatsSection,
    [
        section(
            "Indicators",
            [
                show_read_markers: bool,
                send_read_markers: bool,
                spacer(),
                show_typing_indicators: bool,
                send_typing_indicators: bool,
            ]
        ),
        section(
            "Message",
            [
                url_previews_default: bool,
                mark_pinned_messages: bool,
                system_messages_to_show:
                    enumset::EnumSet<deplace_core::settings::SystemMessageType>,
            ]
        ),
    ]
);

section!(UpdatesSection, []);
