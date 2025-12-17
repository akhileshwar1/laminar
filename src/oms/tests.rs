use rust_decimal_macros::dec;
use crate::oms::order::*;
use crate::oms::core::{OmsCore, Quantity};

#[test]
fn basic_reconciliation() {
    let mut oms = OmsCore::new();

    oms.set_target_position(Quantity(dec!(1.0)));
    assert_eq!(oms.delta().0, dec!(1.0));

    oms.add_open_order(Quantity(dec!(1.0)));
    assert_eq!(oms.delta().0, dec!(0.0));

    oms.on_fill(Quantity(dec!(0.4)));
    assert_eq!(oms.delta().0, dec!(-0.4));

    oms.set_target_position(Quantity(dec!(0.2)));
    assert_eq!(oms.delta().0, dec!(-1.2));
}

#[test]
fn flip_direction() {
    let mut oms = OmsCore::new();

    oms.set_target_position(Quantity(dec!(1.0)));
    oms.add_open_order(Quantity(dec!(1.0)));

    oms.on_fill(Quantity(dec!(1.0)));
    oms.remove_open_order(Quantity(dec!(1.0)));

    oms.set_target_position(Quantity(dec!(-0.5)));
    assert_eq!(oms.delta().0, dec!(-1.5));

}

#[test]
fn partial_fill_flow() {
    let mut o = Order::new(Side::Buy, dec!(1.0));
    o.on_accepted();

    o.on_fill(dec!(0.4));
    assert_eq!(
        o.state,
        OrderState::PartiallyFilled { remaining: dec!(0.6) }
    );

    o.on_fill(dec!(0.6));
    assert_eq!(o.state, OrderState::Filled);
}

#[test]
fn cancel_flow() {
    let mut o = Order::new(Side::Sell, dec!(2.0));
    o.on_accepted();
    o.on_cancel_requested();
    o.on_cancel_confirmed();

    assert_eq!(o.state, OrderState::Cancelled);
}
