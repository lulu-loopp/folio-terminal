# m1.8f-r2:BT_PERF_TRACE 帧内容指纹(小件,high,照单实现)

## 目的

白屏案(m1.8f,提交 0c70f84)剩余嫌疑二选一:「发布的帧本身是空的」vs
「帧有内容但字形提交丢失」。现有 trace 无内容摘要判不了。给每条
BT_PERF_TRACE 帧行加内容指纹,用户复现一次即可定案。

## 交付

1. `crates/bt-app` / `crates/bt-render` 的 BT_PERF_TRACE 帧行(frame=N …)
   追加字段:
   - `nonblank_cells=<count>`:text 非空且非纯空格的单元数
   - `first_text_row=<r>` / `last_text_row=<r>`(无内容时 -1)
   - `content_fnv=<hex>`:对 (row, column, text, fg, bg) 流的 FNV-1a 64
   - `alt=<0|1>`:当前是否备用屏
   计算只在 trace 开启时进行(关闭零成本纪律不变);单帧计算量 O(cells),
   6k 单元量级,开销可忽略但仍要计入 total 之外单列(digest_us=)。
2. `skip=unchanged` 行同样追加 `content_fnv`(基于被跳过的候选帧),
   便于对齐"跳过的帧到底长什么样"。
3. bt-replay --render 的逐帧输出(BT_PERF_TRACE=1 时)同样带指纹,保证
   无头回放与真窗口可对账。
4. 一个单测:构造已知内容帧,断言 nonblank/first/last/fnv 的确定值;
   构造全空帧断言 nonblank=0。

## 边界

- 不改任何呈现/发布/缓存语义——这是纯只读插桩。
- 不动 vendor、glyphon、m1.8e 行缓存、m1.8f 矩阵。
- 门禁:cargo test --workspace --locked、clippy --all-targets -D warnings、
  fmt --check。
- 结果(改动清单+门禁数字+示例 trace 行)直接写在你的最终答复里
  (沙箱写不了工作区外的文件,不必尝试)。停下等审,不提交。
