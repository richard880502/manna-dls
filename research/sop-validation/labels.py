"""Fixed label sets from the product spec (學習單產生器_教師選項與SOP整合規格_20261114).

The display labels of spec §5 must never be renamed, merged or split, so every schema enum and
every check imports them from here.
"""

# §5.1 素材類型
MATERIAL_TYPES = ["閱讀文本", "聽力素材", "影片素材", "圖像素材", "資料／數據"]

# §5.2 學生任務 (grouped only for display)
STUDENT_TASK_GROUPS = {
    "語言與內容學習": ["理解內容", "字彙練習", "文法／句型練習"],
    "思考與資訊處理": ["比較／分類", "推論／預測", "評估／提出理由", "蒐集資料", "整理／分析資料"],
    "互動與合作": ["討論／分享", "小組共同完成"],
    "表達與成果": ["寫作", "製作成果", "成果發表"],
    "反思與評量": ["學習反思", "自我評量", "同儕互評"],
}
STUDENT_TASKS = [t for g in STUDENT_TASK_GROUPS.values() for t in g]

# §5.3 作答方式
ANSWER_FORMATS = ["填空題", "選擇題", "是非題", "配對題", "簡答題", "表格整理", "圖像組織", "段落寫作"]

# §5.4 產生設定
GENERATION_SETTINGS = ["加入暖身活動", "顯示學習目標", "提供引導問題", "顯示作答示例", "提供字詞表",
                       "提供句型框架", "產生基礎版／一般版／進階版"]
VERSIONS = ["基礎版", "一般版", "進階版"]
VERSION_KEYS = {"basic": "基礎版", "standard": "一般版", "advanced": "進階版"}

# §2 資訊處理功能
INFO_FUNCTIONS = ["發現資訊", "整理資訊", "分析資訊", "比較資訊", "創造資訊"]

# §8 資訊處理功能 -> 可參考的學生任務標籤 (a reference relation, not a restriction)
FUNCTION_TASKS = {
    "發現資訊": ["理解內容", "蒐集資料"],
    "整理資訊": ["比較／分類", "整理／分析資料"],
    "分析資訊": ["推論／預測", "評估／提出理由", "整理／分析資料"],
    "比較資訊": ["比較／分類"],
    "創造資訊": ["寫作", "製作成果", "成果發表"],
}

# §3 三種鷹架 and the examples the spec lists for each
SCAFFOLD_TYPES = {
    "語言鷹架": ["改述或重述", "思考放聲", "提示與追問", "透過上下文解釋詞義",
                 "引導問題", "字詞表", "句型框架", "作答示例"],
    "教學鷹架": ["圖像組織", "部分完成的大綱或內容", "調整文本", "標示關鍵概念或字彙"],
    "程序鷹架": ["清楚的分步指示及完成順序", "個別或小組指導安排", "策略性配對或分組",
                 "逐步減少支持"],
}

# Labels the spec explicitly forbids as options (§5, §6, §10.2)
FORBIDDEN_LABELS = ["課綱議題", "SDGs", "SEL", "探究", "高層次思考", "專題任務", "合作活動"]

SIOP_OBJECTIVE_TYPES = ["內容目標", "語言目標"]
