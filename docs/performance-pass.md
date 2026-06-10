# Performance pass

Goal: keep AutoHotkey limited to tray, GUI, hotkeys, and final text delivery.

Move repeated waiting, parsing, and CPU-heavy work into Rust threads where practical.

Measure helper launch overhead, idle wakeups, and typing callback allocations before and after changes.
