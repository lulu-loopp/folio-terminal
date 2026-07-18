# ui-mockup.html 逻辑漏洞排雷总清单(2026-07-16)

来源:3 个 opus 子系统审查(flyout/布局/生命周期)+ 1 个 Codex 全局审查,交叉去重。
状态:☑ 已修(Playwright 实测通过)/ ✖ 不修(注明理由)。修复均于同日完成,回归 0 console error / 0 控制字符。

## A. 崩溃与数据丢失(最高优先)

| # | 状态 | 严重度 | 问题 | 来源 |
|---|---|---|---|---|
| A1 | ☑ | 高 | revivePlan 重建 split 不分配 id → 恢复后拖任意 divider 抛异常。修:revivePlan 给 split 补 `id: splitSeq++` | Codex#1 |
| A2 | ☑ | 高 | floatFilesPane 在纯 files tab 上:唯一 tab 时 pane 与浮窗并存;多 tab 时浮窗持有已删 wsId,Dock 抛异常。修:唯一 tab 时以默认 shell 替换 tab 内容(同 bootFresh 惯例);Dock 对已死 wsId 回退到当前 tab | Codex#2 |
| A3 | ☑ | 高 | runCommand 1.6s 定时器无 token:Refresh 后旧输出灌进新 shell;关窗后回调访问空 activeTab 抛异常。修:session.runGen 世代计数 + sessions 存活检查 + isVisible 判空 | opus-L1 + Codex#6 |
| A4 | ☑ | 高 | restore 提示未响应时再关窗:pendingRestore 被覆盖丢失、对话框留在桌面态。修:setShut 把未答复计划并回 lastSession 并收起对话框 | opus-L3 + Codex#5 |
| A5 | ☑ | 高/中 | recordRecent 跳过纯 files tab → 关闭即永久丢失。修:Recent 支持 files 条目(root/open/sel/width),reopenRecent/菜单渲染同步支持 | opus-L2 |

## B. 布局与拖拽正确性

| # | 状态 | 严重度 | 问题 | 来源 |
|---|---|---|---|---|
| B1 | ☑ | 高 | 中心 drop 预览按 replace 建模、提交是 swap。修:planDrop center 对 pane 拖拽双向建模(target←moving 载荷,moving←target 载荷);实测预览与提交逐像素一致 | opus-Y1 + Codex#8 |
| B2 | ☑ | 中 | 双固定 row:renderNode 留死白,rectsOf 算 b=剩余,两路分歧。修:renderNode 双固定时 b 用 `flex:1 0 fb`(基准 fb、可吸收盈余),与 rectsOf 同一语义,死白消失 | opus-Y2 + Codex#9 |
| B3 | ☑ | 中 | 纯 files 的 col 子树丢失固定宽语义。修:rowFixedWidth 对全固定 col 返回 max(a,b) | Codex#10 |
| B4 | ☑ | 高 | 整 tab 拖拽预览只画单叶,提交插入整棵子树。修:planDrop 接受 srcTree,预览画真实脚印(accent 为子树包围盒),planFits 按真实叶子判定(窄窗口正确拒绝) | Codex#3 |
| B5 | ☑ | 高 | 外层 divider 只按单 pane 最小值夹取,内层终端可被压到 ~130px。修:新增 minSizeAlong 子树需求(同向求和+divider、异向取 max),divider 夹取按两侧子树需求 | Codex#4 |
| B6 | ☑ | 低 | 相邻两 files 列 divider:b 被内联拉伸、宽度丢失。修:邻侧为 files 时不再内联 `flex:1 1 0`,拖拽只写 a 的绝对宽,pointerup 全量 render 对账;夹取下限按邻侧真实最小值 | opus-Y4 |
| B7 | ☑ | 低 | files 列 pop-out→Dock 丢自定义宽。修:float/Dock 的 seed 链路携带 width | opus-Y5 |
| B8 | ☑ | 低 | movePane 防御分支静默丢弃已 detach 的叶子。修:target 消失时改挂根边缘 | opus-Y7 |
| B9 | ✖ | 低 | miniNode 缩略图按 ratio 不按固定宽 —— **用户已裁决保持 ratio 原样,不修** | opus-Y3 + Codex#20 |

附注(B6):固定子树不是裸 files 叶(如上下堆叠的 files 组)时,其相邻 divider 现在拒绝拖动(此前会把 flex-grow 写到固定槽上造成布局损坏);单叶宽度请拖它自己的 divider。

## C. flyout / 菜单 / 键盘 / 拖拽状态机

| # | 状态 | 严重度 | 问题 | 来源 |
|---|---|---|---|---|
| C1 | ☑ | 高/中 | closeTab/selectTab 无条件摧毁 pinned 浮窗。修:两处只关 transient;selectTab 重选当前 tab 变 no-op;Dock 对已死来源 tab 回退当前 tab。契约(×/Esc/Dock/re-click 才关)现与实现一致 | opus-F1 + Codex#13 |
| C2 | ☑ | 中 | 弹出层无集中互斥(contextmenu 绕过 click 关闭器、stopPropagation 互拦、root 菜单双 chevron、Recent 陈旧索引)。修:closeAllPopups() 由所有打开路径调用;closeTab 也调用(清陈旧引用) | opus-F2/F3 + Codex#16/#17 |
| C3 | ☑ | 中 | flyCloseT 到期不归零 → 第二次 peek 永不自动关;Esc 不 disarm flyOpenT → 关了又弹。修:超时回调先置 null;closeFlyout 统一 disarm/cancel 两个计时器 | Codex#11/#12 |
| C4 | ☑ | 中 | Esc 不取消拖拽、且一次全关所有层。修:Esc 分层路由(拖拽/resize 取消 → term/root/profile 菜单 → combo → settings → flyout,每按一次关一层);cancelDrag 不提交 drop,cancelResizeDrag 还原原值 | Codex#18 + opus-F4 |
| C5 | ☑ | 中 | settings 遮罩(z-40)低于 flyout/菜单(z-55/60)、Ctrl+B/Ctrl+Shift+T 穿透、菜单开着时打字进终端。修:遮罩升 z-70;两个快捷键加 settings 守卫;typing 路径加菜单守卫 | Codex#7/#15 + opus-F8 |
| C6 | ☑ | 中 | 双击 `.tab-files` 误触重命名。修:dblclick 排除名单加入 `.tab-files` | opus-F5 + Codex#19 |
| C7 | ☑ | 中 | divider 拖拽期间 render() → 持悬空 DOM。修:pointermove 检查 splitEl.isConnected,断连即 endResize | Codex#14 |
| C8 | ☑ | 低/中 | pinned 浮窗无 resize 重钳位,可能飘出视口。修:window resize 监听重钳 left/top | opus-F6 |
| C9 | ☑ | 低 | root 菜单开着时 render 后与锚点错位/悬空。修:render 末尾 syncRootMenu() 跟随活锚点或折叠(chevron 本就由 filesPaneHtml 重derive,该部分原报告有误) | opus-F7 |
| C10 | ☑ | 低 | hover intent 计时器绑死旧 DOM,180ms 内 render 则静默失效。修:liveTrig() 按 leaf/tab 身份重解析触发器 | Codex#21 |
| C11 | ☑ | 中(疑似) | 拖拽无 pointercancel 清理路径。修:flyout 拖拽/全局 drag/resize 均补 pointercancel 监听 | Codex#22 |

## D. 序列化完整性

| # | 状态 | 严重度 | 问题 | 来源 |
|---|---|---|---|---|
| D1 | ☑ | 中 | 不序列化 active tab。修:planOf 记 active 标志,launch/finishLaunch 恢复(pinned 活动 tab 不被 restore 抢走) | opus-L4 |
| D2 | ☑ | 中/低 | 不序列化 per-tab focus。修:planOf 记 focusIndex(叶序索引),revivePlan 还原 | opus-L5 |
| D3 | ☑ | 低 | 重命名未回车时 shut 丢新名字。修:setShut 先对 .rename 强制 blur(同步提交)再序列化 | opus-L6 |

## 复核确认无问题(四方一致,免重复排查)

uid/id 无碰撞与残留;Recent 不引用死会话;重复 restore 不翻倍;最后一个 tab/pane 保护;refresh/clear 对 files 叶有守卫;pinned 分区拖拽重排有 guard;单固定列 renderNode/rectsOf 一致;拖到自身有 guard;极小窗口不除零不崩;DIVIDER_PX 三处一致;peekHit rect 不因 render 失效;菜单监听无堆叠;pinned 期间 hover 控制器静默;setShut 清 flyout。

## Playwright 验证记录(2026-07-16)

五批实测全过:①A1/A4/A5/D1/D2(shut→restore 循环、files Recent 重开);②B1-B5(swap 预览=提交逐像素、双固定无死白且两路一致、col 固定宽=max、整 tab 拖拽 3 叶脚印+窄窗口拒绝、子树需求 521/260/260);③C1/C2/A2/A3(pinned 浮窗五连生存、弹层互斥、Esc 分层、幽灵输出隔离、纯 files tab 浮出);④关窗竞态、二次 peek 自关、视口重钳、菜单/settings 键盘守卫;⑤真实鼠标 Esc 取消拖拽(预览/ghost 清理、树不变、正常拖放不受影响)。回归:0 console error,0 控制字符。
