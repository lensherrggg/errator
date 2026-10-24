use crate::model::{Issue, Severity, Source};
use crate::rules::{Rule, RuleContext};

#[derive(Debug, Clone)]
pub struct DictionaryEntry {
    pub wrong: String,
    pub right: String,
    pub severity: Option<Severity>,
}

impl DictionaryEntry {
    fn new(wrong: &str, right: &str) -> Self {
        Self {
            wrong: wrong.to_owned(),
            right: right.to_owned(),
            severity: None,
        }
    }

    pub fn with_severity(wrong: &str, right: &str, severity: Option<Severity>) -> Self {
        Self {
            wrong: wrong.to_owned(),
            right: right.to_owned(),
            severity,
        }
    }
}

pub struct ConfusionRule {
    id: String,
    category: String,
    severity: Severity,
    description: String,
    source: Source,
    confidence: f32,
    entries: Vec<DictionaryEntry>,
}

impl ConfusionRule {
    pub fn builtin() -> Self {
        Self {
            id: "TYPO_CONFUSION".to_owned(),
            category: "typography".to_owned(),
            severity: Severity::Warning,
            description: "常见错别字与词语混淆（内置词典）".to_owned(),
            source: Source::Dictionary,
            confidence: 0.95,
            entries: builtin_entries(),
        }
    }

    pub fn custom(entries: Vec<DictionaryEntry>) -> Self {
        Self {
            id: "CUSTOM_DICTIONARY".to_owned(),
            category: "dictionary".to_owned(),
            severity: Severity::Warning,
            description: "用户自定义词库".to_owned(),
            source: Source::Dictionary,
            confidence: 1.0,
            entries,
        }
    }
}

impl Rule for ConfusionRule {
    fn id(&self) -> &str {
        &self.id
    }

    fn category(&self) -> &str {
        &self.category
    }

    fn default_severity(&self) -> Severity {
        self.severity
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn check(&self, context: &RuleContext<'_>) -> Vec<Issue> {
        let document = context.document;
        let mut issues = Vec::new();
        for entry in &self.entries {
            if entry.wrong.is_empty() {
                continue;
            }
            for (offset, _) in document.text.match_indices(&entry.wrong) {
                if context.cancelled() {
                    return issues;
                }
                if document.is_masked(offset, entry.wrong.len()) {
                    continue;
                }
                issues.push(context.issue(
                    self.id(),
                    self.category(),
                    entry.severity.unwrap_or_else(|| self.default_severity()),
                    offset,
                    entry.wrong.clone(),
                    entry.right.clone(),
                    format!("「{}」疑为「{}」的常见误写", entry.wrong, entry.right),
                    self.source,
                    self.confidence,
                ));
            }
        }
        issues
    }
}

fn builtin_entries() -> Vec<DictionaryEntry> {
    const PAIRS: &[(&str, &str)] = &[
        ("按装", "安装"),
        ("以经", "已经"),
        ("因该", "应该"),
        ("既使", "即使"),
        ("侯车室", "候车室"),
        ("侯选人", "候选人"),
        ("寒喧", "寒暄"),
        ("松驰", "松弛"),
        ("精采", "精彩"),
        ("凑和", "凑合"),
        ("隐密", "隐秘"),
        ("恶梦", "噩梦"),
        ("渡假", "度假"),
        ("蜇伏", "蛰伏"),
        ("座落", "坐落"),
        ("布署", "部署"),
        ("竟争", "竞争"),
        ("竞然", "竟然"),
        ("在次", "再次"),
        ("做为", "作为"),
        ("莫明其妙", "莫名其妙"),
        ("谈笑风声", "谈笑风生"),
        ("甘败下风", "甘拜下风"),
        ("一如继往", "一如既往"),
        ("不知所错", "不知所措"),
        ("穿流不息", "川流不息"),
        ("出奇不意", "出其不意"),
        ("竞竞业业", "兢兢业业"),
        ("走头无路", "走投无路"),
        ("一愁莫展", "一筹莫展"),
        ("迫不急待", "迫不及待"),
        ("人才倍出", "人才辈出"),
        ("再接再励", "再接再厉"),
        ("阴谋鬼计", "阴谋诡计"),
        ("迫在眉捷", "迫在眉睫"),
        ("肆无忌殚", "肆无忌惮"),
        ("别出新裁", "别出心裁"),
        ("英雄倍出", "英雄辈出"),
        ("不径而走", "不胫而走"),
        ("弱不经风", "弱不禁风"),
        ("一股作气", "一鼓作气"),
        ("拾金不味", "拾金不昧"),
        ("声名雀起", "声名鹊起"),
        ("挖墙角", "挖墙脚"),
        ("水笼头", "水龙头"),
        ("好象", "好像"),
        ("图象", "图像"),
        ("宏扬", "弘扬"),
        ("针贬", "针砭"),
        ("脉膊", "脉搏"),
        ("防碍", "妨碍"),
        ("入场卷", "入场券"),
        ("记帐", "记账"),
        ("另人", "令人"),
        ("陷井", "陷阱"),
        ("末来", "未来"),
        ("带替", "代替"),
        ("付责", "负责"),
        ("按耐", "按捺"),
        ("安祥", "安详"),
        ("报筹", "报酬"),
        ("必竟", "毕竟"),
        ("沉缅", "沉湎"),
        ("固步自封", "故步自封"),
        ("好高鹜远", "好高骛远"),
        ("既往不究", "既往不咎"),
        ("娇揉造作", "矫揉造作"),
        ("金榜提名", "金榜题名"),
        ("九宵云外", "九霄云外"),
        ("老奸巨滑", "老奸巨猾"),
        ("礼上往来", "礼尚往来"),
        ("名列前矛", "名列前茅"),
        ("明火执杖", "明火执仗"),
        ("默守成规", "墨守成规"),
        ("沤心沥血", "呕心沥血"),
        ("破斧沉舟", "破釜沉舟"),
        ("千锤百练", "千锤百炼"),
        ("如火如茶", "如火如荼"),
        ("世外桃园", "世外桃源"),
        ("首曲一指", "首屈一指"),
        ("同仇敌慨", "同仇敌忾"),
        ("图穷匕现", "图穷匕见"),
        ("万事具备", "万事俱备"),
        ("妄费心机", "枉费心机"),
        ("望而怯步", "望而却步"),
        ("相形见拙", "相形见绌"),
        ("消声匿迹", "销声匿迹"),
        ("心浮气燥", "心浮气躁"),
        ("兴高彩烈", "兴高采烈"),
        ("虚无缥渺", "虚无缥缈"),
        ("宣宾夺主", "喧宾夺主"),
        ("一诺千斤", "一诺千金"),
        ("一泄千里", "一泻千里"),
        ("义愤填鹰", "义愤填膺"),
        ("饮鸠止渴", "饮鸩止渴"),
        ("有条不稳", "有条不紊"),
        ("怨天由人", "怨天尤人"),
        ("责无旁代", "责无旁贷"),
        ("振聋发馈", "振聋发聩"),
        ("中流抵柱", "中流砥柱"),
        ("转弯摸角", "转弯抹角"),
        ("自抱自弃", "自暴自弃"),
        ("帐号", "账号"),
        ("帐户", "账户"),
        ("严竣", "严峻"),
        ("峻工", "竣工"),
        ("候复", "回复"),
        ("蓝球", "篮球"),
        ("竟赛", "竞赛"),
        ("竟选", "竞选"),
        ("窜改", "篡改"),
        ("编篡", "编纂"),
        ("串改", "篡改"),
        ("大姆指", "大拇指"),
    ];

    PAIRS
        .iter()
        .map(|(wrong, right)| DictionaryEntry::new(wrong, right))
        .collect()
}
