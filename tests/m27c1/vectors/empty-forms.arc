pub component EmptyComponent {}
pub resource EmptyResource {}
pub struct EmptyRecord {}
pub struct EmptyTuple();
pub enum EmptyEnum {}
pub trait EmptyTrait {}
impl EmptyRecord {}
pub schedule EmptySchedule {}
pub system EmptyQuery(empty: query []) requires {} throws {} {}
pub world EmptyWorld {
    init {}
}

pub fn empty_values() {
    let unit = ();
    let array = [];
    let tuple = (unit,);
    let record = EmptyRecord {};
    let closure = || requires {} throws {} ();
}
