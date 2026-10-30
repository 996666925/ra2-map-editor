//! Display text only. INI keys, resource IDs and editable values stay unchanged.
use std::borrow::Cow;

/// Keep unfamiliar extension fields readable under their original name.
pub(crate) fn field(key: &str) -> &str {
    match key {
        "Name" => "名称",
        "Author" => "作者",
        "Player" => "玩家所属方",
        "House" => "所属方",
        "Country" => "国家",
        "Color" => "颜色",
        "PlayerControl" => "玩家控制",
        "IQ" => "智能等级",
        "Credits" => "初始资金",
        "TechLevel" => "科技等级",
        "Allies" => "盟友列表",
        "Edge" => "增援进入方向",
        "ActsLike" => "行为参照国家",
        "PercentBuilt" => "基地预建比例",
        "NodeCount" => "基地节点数量",
        "MultiplayerOnly" => "仅限多人游戏",
        "HomeCell" => "初始镜头位置",
        "AltHomeCell" => "备用镜头位置",
        "Intro" => "开场影片",
        "Brief" => "简报影片",
        "Action" => "任务开始影片",
        "Theme" => "背景音乐",
        "Win" => "胜利影片",
        "Lose" => "失败影片",
        "NextScenario" => "下一关地图",
        "AltNextScenario" => "分支关卡地图",
        "SkipMapSelect" => "跳过地图选择",
        "TimerInherit" => "继承倒计时",
        "CarryOverMoney" => "资金继承比例",
        "CarryOverCap" => "资金继承上限",
        "PostScore" => "计分后影片",
        "PreMapSelect" => "选关前影片",
        "EndOfGame" => "战役终关",
        "OneTimeOnly" => "仅执行一次",
        "SkipScore" => "跳过计分画面",
        "Script" => "脚本",
        "TaskForce" => "特遣部队",
        "Trigger" => "触发器",
        "链接 Trigger" => "链接触发器",
        "Tag" => "标签",
        "Group" => "分组",
        "Waypoint" => "起始航点",
        "TransportWaypoint" => "运输航点",
        "Max" => "最大同时存在数量",
        "Priority" => "优先级",
        "VeteranLevel" => "老兵等级",
        "MindControlDecision" => "心灵控制决策",
        "Autocreate" => "自动创建",
        "Full" => "满编出动",
        "Recruiter" => "允许招募",
        "Prebuild" => "预先建造",
        "Reinforce" => "允许增援",
        "Suicide" => "自杀式任务",
        "Aggressive" => "主动攻击",
        "Annoyance" => "骚扰小队",
        "AreTeamMembersRecruitable" => "成员可被招募",
        "AvoidThreats" => "避开威胁",
        "Droppod" => "空降舱投放",
        "GuardSlower" => "降低警戒速度",
        "IonImmune" => "免疫闪电风暴",
        "IsBaseDefense" => "基地防御小队",
        "IsTilter" => "允许倾斜",
        "LooseRecruit" => "宽松招募",
        "OnTransOnly" => "仅使用运输单位",
        "OnlyTargetHouseEnemy" => "仅攻击所属方的敌人",
        "TransportsReturnOnUnload" => "卸载后运输单位返回",
        "UseTransportOrigin" => "使用运输起点",
        "Whiner" => "遇袭求援",
        "Triggers" => "触发器设置",
        "Events" => "事件",
        "Actions" => "动作",
        "AITriggerTypes" => "电脑触发器定义",
        "AITriggerTypesEnable" => "电脑触发器启用状态",
        "VariableNames" => "局部变量",
        "EditorGlobalNames" => "全局变量注释",
        "Structures" => "建筑",
        "Units" => "车辆",
        "Infantry" => "步兵",
        "Aircraft" => "飞机",
        "UIName" => "显示名称",
        "Briefing" => "任务简报",
        "LSLoadMessage" => "载入标题",
        "LSLoadBriefing" => "载入简报",
        "Ambient" | "IonAmbient" | "DominatorAmbient" => "环境亮度",
        "Red" | "IonRed" | "DominatorRed" => "红色分量",
        "Green" | "IonGreen" | "DominatorGreen" => "绿色分量",
        "Blue" | "IonBlue" | "DominatorBlue" => "蓝色分量",
        "Ground" | "IonGround" | "DominatorGround" => "地面亮度补偿",
        "Level" | "IonLevel" | "DominatorLevel" => "每层高度亮度补偿",
        "DominatorAmbientChangeRate" => "心灵控制器光照过渡间隔",
        "NukeAmbientChangeRate" => "核爆光照过渡间隔",
        _ => key,
    }
}

/// Translate known terrain words without guessing names supplied by mods.
pub(crate) fn terrain(name: &str) -> Cow<'_, str> {
    let mut translated = String::new();
    for word in name.split_whitespace() {
        let lower = word.to_ascii_lowercase();
        let label = match lower.as_str() {
            "clear" => "空地",
            "rough" => "粗糙地面",
            "sand" => "沙地",
            "green" | "grass" => "草地",
            "dirt" => "泥地",
            "pave" | "pavement" | "paved" => "铺装",
            "road" | "roads" => "道路",
            "highway" | "highways" => "公路",
            "rail" | "railroad" | "railroads" => "铁路",
            "track" | "tracks" => "轨道",
            "cliff" | "cliffs" => "悬崖",
            "ramp" | "ramps" => "坡道",
            "slope" | "slopes" => "斜坡",
            "water" => "水面",
            "shore" | "shores" => "水岸",
            "beach" | "beaches" => "沙滩",
            "waterfall" | "waterfalls" => "瀑布",
            "bridge" | "bridges" => "桥梁",
            "wood" | "wooden" => "木质",
            "concrete" => "混凝土",
            "destroyable" | "destructible" => "可破坏",
            "cave" | "caves" => "洞穴",
            "tunnel" | "tunnels" => "隧道",
            "entrance" | "entrances" => "入口",
            "ice" => "冰面",
            "snow" => "积雪",
            "rock" | "rocks" => "岩石",
            "lat" => "自动过渡",
            "to" => "过渡至",
            "transition" | "transitions" => "过渡",
            "misc" | "misc." | "miscellaneous" => "杂项",
            "fix" | "fixes" => "修补",
            "end" | "ends" => "末端",
            "piece" | "pieces" => "拼块",
            "median" | "medians" => "隔离带",
            "junction" | "junctions" => "交叉口",
            "curve" | "curves" => "弯道",
            "straight" => "直线",
            "small" => "小型",
            "large" => "大型",
            "low" => "低位",
            "high" => "高位",
            "broken" | "damaged" => "损坏",
            "slopepieces" => "斜坡拼块",
            "north" => "北向",
            "south" => "南向",
            "east" => "东向",
            "west" => "西向",
            _ if word
                .bytes()
                .all(|b| b.is_ascii_digit() || b"-+/()".contains(&b)) =>
            {
                word
            }
            _ => return Cow::Borrowed(name),
        };
        translated.push_str(label);
    }
    if translated.is_empty() {
        Cow::Borrowed(name)
    } else {
        Cow::Owned(translated)
    }
}

pub(crate) fn terrain_brush(label: &str) -> Cow<'_, str> {
    if let Some((group, detail)) = label.split_once(" · ")
        && let Cow::Owned(group) = terrain(group)
    {
        return Cow::Owned(format!("{group} · {detail}"));
    }
    Cow::Borrowed(label)
}
