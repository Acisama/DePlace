use chrono::{DateTime, Local};

pub fn format_message_date(date: DateTime<Local>) -> String {
    // let timezone = settings.timezone.tracked();
    // let date = date.with_timezone(&timezone);
    // let now = Local::now().with_timezone(&timezone);

    // let hour_str = match settings.hour_format.tracked() {
    //     HourFormat::TwelveHour => "%I:%M %p",
    //     HourFormat::TwentyFourHour => "%H:%M",
    // };
    // let date_str = match settings.date_format.tracked() {
    //     DateFormat::DayMonthYear => "%d/%m/%Y",
    //     DateFormat::MonthDayYear => "%m/%d/%Y",
    //     DateFormat::YearMonthDay => "%Y/%m/%d",
    // };

    let hour_str = "%H:%M";
    let date_str = "%d/%m/%Y";
    let now = Local::now();

    match (date.date_naive() - now.date_naive()).num_days() {
        0 => date.format(&format!("Today, {}", hour_str)).to_string(),
        -1 => date.format(&format!("Yesterday, {}", hour_str)).to_string(),
        -6..-1 => date
            .format(&format!("%a {}, {}", date_str, hour_str))
            .to_string(),
        _ => date
            .format(&format!("{}, {}", date_str, hour_str))
            .to_string(),
    }
}

pub fn format_date_divider(date: DateTime<Local>) -> String {
    let now = Local::now();

    let is_today = date.date_naive() == now.date_naive();
    let is_yesterday = date.date_naive() == (now - chrono::Duration::days(1)).date_naive();
    let is_week_ago = date > now - chrono::Duration::days(7);

    if is_today {
        "Today".to_string()
    } else if is_yesterday {
        "Yesterday".to_string()
    } else {
        date.format(&format!("{}%d %B %Y", if is_week_ago { "%a " } else { "" }))
            .to_string()
    }
}

pub fn format_message_sent_time(date: DateTime<Local>) -> String {
    date.format("%H:%M").to_string()
}
