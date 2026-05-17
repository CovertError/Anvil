use anvil_core::schedule::Schedule;

pub fn build() -> Schedule {
    let schedule = Schedule::new();
    // Examples:
    //   schedule.daily_at("02:00", Arc::new(MyTask));
    //   schedule.hourly(Arc::new(OtherTask));
    schedule
}
