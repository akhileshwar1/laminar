use ratatui::{
    Frame,
    layout::{Layout, Constraint, Direction},
    widgets::{Block, Borders, Paragraph, Table, Row},
};

use crate::tui::app::TuiApp;

pub fn draw(f: &mut Frame, app: &TuiApp) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),  // strategy
            Constraint::Min(10),    // orders
            Constraint::Length(7),  // position
        ])
        .split(f.size());

    // --- STRATEGY ---
    let strategy = Paragraph::new(format!(
            "Mid: {}\nSpread: {}\nSkew: {}\nBid: {}\nAsk: {}",
            app.mid, app.spread, app.skew, app.bid, app.ask
    ))
        .block(Block::default().title("Strategy").borders(Borders::ALL));
    f.render_widget(strategy, chunks[0]);

    // --- ORDERS ---
    let rows = app.snapshot.iter().flat_map(|s| &s.orders).map(|o| {
        Row::new(vec![
            o.id.to_string(),
            format!("{:?}", o.side),
            o.limit_price.to_string(),
            o.original_qty.to_string(),
            o.remaining_qty.to_string(),
            format!("{:?}", o.state),
        ])
    });
    let table = Table::new(
        rows,
        [
        Constraint::Length(36), // OrderId
        Constraint::Length(4),  // Side
        Constraint::Length(10), // Price
        Constraint::Length(10), // Orig qty
        Constraint::Length(10), // Remaining
        Constraint::Length(16), // State
        ],
    );
    // let table = Table::new(rows)
    //     .header(Row::new(["ID", "Side", "Px", "Qty", "rem_qty", "State"]))
    //     .block(Block::default().title("Live Orders").borders(Borders::ALL))
    //     .widths(&[
    //         Constraint::Length(36),
    //         Constraint::Length(6),
    //         Constraint::Length(10),
    //         Constraint::Length(8),
    //         Constraint::Length(8),
    //         Constraint::Min(10),
    //     ]);

    f.render_widget(table, chunks[1]);

    // --- POSITION ---
    if let Some(s) = &app.snapshot {
        let pos = Paragraph::new(format!(
                "Net: {}\nAvg Px: {}",
                s.net_position, s.avg_price
        ))
            .block(Block::default().title("Position").borders(Borders::ALL));
        f.render_widget(pos, chunks[2]);
    }
}
