use macros::section;

section!(
    GeneralSection,
    [
        section(
            "Language/Region",
            [
                hour_format,
                date_format,
                spacer(),
                first_day_of_week,
                timezone,
            ]
        ),
        section("Units", [data_size_unit]),
        section("Behavior", [minimize_to_tray]),
        section(
            "Profile customization",
            [play_user_theme_on_click, use_banner_colors, use_name_color,]
        ),
    ]
);

section!(AppearanceSection, []);

section!(AudioSection, []);

section!(
    ChatsSection,
    [
        section(
            "Indicators",
            [
                show_read_markers,
                send_read_markers,
                spacer(),
                show_typing_indicators,
                send_typing_indicators,
            ]
        ),
        section(
            "Message",
            [
                url_previews_default,
                mark_pinned_messages,
                system_messages_to_show
            ]
        ),
    ]
);

section!(UpdatesSection, []);
