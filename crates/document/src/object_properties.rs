//! The authored CSV columns read by VERA20k's per-category scenario readers.
#[derive(Clone, Copy)]
pub enum Kind {
    Text,
    Number(i32, i32),
    Flag,
}
#[derive(Clone, Copy)]
pub struct Field {
    pub index: usize,
    pub name: &'static str,
    pub kind: Kind,
}
use Kind::*;
pub fn fields(section: &str) -> Vec<Field> {
    let mut result = Vec::new();
    let mut add = |index, name, kind| result.push(Field { index, name, kind });
    match section {
        "Structures" => {
            add(6, "关联标签", Text);
            add(7, "AI 可出售", Flag);
            add(8, "AI 重建", Flag);
            add(9, "启用供电", Flag);
            add(10, "升级数量", Number(0, 3));
            add(12, "升级 1", Text);
            add(13, "升级 2", Text);
            add(14, "升级 3", Text);
            add(15, "AI 可维修", Flag);
        }
        "Infantry" => {
            add(5, "步兵子格", Number(0, 4));
            add(6, "初始任务", Text);
            add(8, "关联标签", Text);
            add(9, "老兵百分比", Number(0, 200));
            add(10, "分组", Number(-1, 9));
            add(11, "桥面 / 高层", Flag);
            add(12, "招募标志 A", Flag);
            add(13, "招募标志 B", Flag);
        }
        "Units" | "Aircraft" => {
            add(6, "初始任务", Text);
            add(7, "关联标签", Text);
            add(8, "老兵百分比", Number(0, 200));
            add(9, "分组", Number(-1, 9));
            if section == "Units" {
                add(10, "桥面 / 高层", Flag);
                add(11, "跟随车辆记录", Text);
                add(12, "招募标志 A", Flag);
                add(13, "招募标志 B", Flag);
            } else {
                add(10, "招募标志 A", Flag);
                add(11, "招募标志 B", Flag);
            }
        }
        _ => {}
    }
    result
}
