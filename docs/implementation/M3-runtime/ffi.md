# C ABI（`tsukumoe-ffi`）

- REQ: REQ-RT-002
- 単位: M3-03
- 状態: 実装済み（3 OS の CI と `unsafe` の検査が残る）
- 依存: M3-02
- クレート: `tsukumoe-ffi`（新設。`tsukumoe-runtime` と `tsukumoe-core` だけに依存する。`unsafe` はこのクレートだけに置く）

## 目的

`Runtime` を C ABI で公開する。Unity（M3-05、別 repo）と、C / C++ から使う利用者が、同じ入口で再生できる。

## 方針

- **不透明ハンドル**: `TsukumoeRuntime*`。作成は `tsukumoe_runtime_load(data, len, &out)`。データは 16 バイト境界でなくてよい。ライブラリが境界に合うバッファへコピーして持つ（呼び出し側のバッファは戻った後に解放してよい）
- **エラー**: すべての関数が `TsukumoeStatus`（`OK`・`NULL_ARGUMENT`・`INVALID_DATA`・`OUT_OF_RANGE`・`NOT_FOUND`・`INTERNAL`（パニック））を返す。詳細な文言は `tsukumoe_last_error_message()`（スレッドごとに保持、UTF-8、次の呼び出しまで有効）
- **パニックを境界で止める**: すべての `extern "C"` を共通の包み（`catch_unwind`）に通す。パニックは `INTERNAL` に変える。テストで強制的にパニックを起こして確かめる
- **所有と寿命**: 返したポインタ（頂点・添字・名前）は、次の `update` / 再生操作 / `destroy` まで有効。呼び出し側は解放しない。文字列は NUL 終端の UTF-8
- **スレッド**: ハンドルは同時に 1 スレッドからだけ使う（複数スレッドで別ハンドルを使うのは可）。仕様に書く
- **版**: `tsukumoe_abi_version()`（メジャー・マイナー）。構造体は固定幅・`repr(C)`。自動動作の設定のように将来足す構造体は、先頭にサイズを持つ
- **命名**: 接頭辞 `tsukumoe_`（ADR-0023）。ヘッダは `include/tsukumoe.h`

## 公開する内容（M3-02 の `Runtime` に対応）

- ロードと破棄、パラメータ（数・名前・範囲・添字の引き当て・設定・解除・現在値）
- モーション（再生・停止、レイヤ・優先度・ループ）、表情、自動動作の設定
- `update(dt)`、描画リストの取得（描画順に並んだ描画対象の数、頂点位置・UV・添字、不透明度・ブレンド・マスク・色・テクスチャ番号・変化の有無）
- 埋め込みテクスチャ（数、PNG のバイト列へのポインタと長さ）

## 範囲

- `tsukumoe-ffi`（`cdylib` + `staticlib` + `rlib`）、`include/tsukumoe.h`（cbindgen で生成）、C のサンプル `examples/c/`（`docs/specifications/samples/quad.tmod` を読み、パラメータを動かして頂点を出力する）、仕様 `docs/specifications/c-abi.md`（寿命・スレッド・エラー・版の規則）
- 範囲外: C++ ラッパ、他言語のバインディング（Unity は M3-05）、モデルの編集 API

## 検査

- ヘッダ: テストが cbindgen（dev 依存）で生成し、保存済みの `include/tsukumoe.h` と比べる。差があれば失敗。`TSUKUMOE_UPDATE_HEADER=1` で更新（スキーマと同じ流儀）
- パニック: 境界で `INTERNAL` に変わり、後続の呼び出しが壊れない
- 不正な引数: NULL・範囲外の添字・壊れた `.tmod`・名前なしで、パニックせずに状態コードを返す
- 一致: C の API の結果が、Rust の `Runtime` を直接使った結果とビット単位で一致する
- C のサンプル: `cc`（dev 依存）でビルドして実行し、出力を検査する。macOS・Ubuntu・Windows(MSVC) で回す

## 受入条件

- [x] ヘッダが自動生成され、CI で差分がないことを検査する
- [x] パニックが FFI 境界を越えない（すべてエラーコードに変換する）
- [ ] C のサンプルプログラムが 3 OS でビルド・実行できる（macOS で確認済み。Ubuntu・Windows は CI で確認する）
- [x] 寿命・スレッド・エラーの規則が仕様にある
- [ ] `unsafe` が `tsukumoe-ffi` だけにある（`deps` タスクで検査）。`tsukumoe-core` のテスト（`eval.rs` の割り当て計数用 `GlobalAlloc`）に `unsafe` があるため検査を入れていない。方針の判断待ち。依存の範囲の検査は `deps` に入れた

## 決めたこと

- cbindgen は dev 依存でテストから呼ぶ（ツールのインストール不要）
- C のサンプルのビルドは `cc` クレート（dev 依存）
- 共有ライブラリ（`cdylib`）にリンクする。静的ライブラリは OS ごとにシステムライブラリの指定が要るため、C サンプルとテストは `cdylib` を使う
- ヘッダ `include/tsukumoe.h`・サンプル `examples/c/` はクレート内（`crates/tsukumoe-ffi/`）に置く
