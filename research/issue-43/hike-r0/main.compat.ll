; ModuleID = 'main'
source_filename = "main.hike"
target triple = "wasm32-unknown-unknown"

; ==============================================================================
; Hike Runtime for WebAssembly (wasm32-unknown-unknown)
; ==============================================================================

; --- 32-bit Allocator Declarations ---
declare noalias i8* @malloc(i32)
declare noalias i8* @calloc(i32, i32)
declare void @free(i8*)

; --- POSIX / WASM Host Threading & Synchronization Imports ---
declare i32 @hike_thread_spawn(void (i8*)*, i8*)
declare i8* @hike_event_create()
declare void @hike_event_signal(i8*)
declare i32 @hike_event_wait(i8*, i32)
declare void @hike_event_destroy(i8*)
declare void @hike_sleep_ms(i32)
declare i64 @hike_now_ns()

; --- 32-bit Task Descriptor: 20 bytes (ptr*4 + i32*1) ---
%struct.__hike_task = type { void (i8*, i8*)*, i8*, i8*, i32, i8* }

; ワーカースレッドのエントリサンク
define internal void @__hike_task_worker_thunk(i8* %param) {
entry:
  %task = bitcast i8* %param to %struct.__hike_task*
  %p_fn = getelementptr inbounds %struct.__hike_task, %struct.__hike_task* %task, i32 0, i32 0
  %fn = load void (i8*, i8*)*, void (i8*, i8*)** %p_fn
  %p_env = getelementptr inbounds %struct.__hike_task, %struct.__hike_task* %task, i32 0, i32 1
  %env = load i8*, i8** %p_env
  %p_buf = getelementptr inbounds %struct.__hike_task, %struct.__hike_task* %task, i32 0, i32 2
  %buf = load i8*, i8** %p_buf

  ; タスク本体を実行
  call void %fn(i8* %env, i8* %buf)

  ; 完了フラグ更新
  %p_done = getelementptr inbounds %struct.__hike_task, %struct.__hike_task* %task, i32 0, i32 3
  store i32 1, i32* %p_done

  ; 起床シグナル発火
  %p_ev = getelementptr inbounds %struct.__hike_task, %struct.__hike_task* %task, i32 0, i32 4
  %ev = load i8*, i8** %p_ev
  %has_ev = icmp ne i8* %ev, null
  br i1 %has_ev, label %do_signal, label %exit
do_signal:
  call void @hike_event_signal(i8* %ev)
  br label %exit
exit:
  ret void
}

; タスクの非同期オフロード
define internal %struct.__hike_task* @__hike_async(i8* %fn_ptr, i8* %env_ptr, i32 %ret_size) {
entry:
  %raw_task = call i8* @malloc(i32 20)
  %task = bitcast i8* %raw_task to %struct.__hike_task*

  %fn_thunk = bitcast i8* %fn_ptr to void (i8*, i8*)*
  %p_fn = getelementptr inbounds %struct.__hike_task, %struct.__hike_task* %task, i32 0, i32 0
  store void (i8*, i8*)* %fn_thunk, void (i8*, i8*)** %p_fn

  %p_env = getelementptr inbounds %struct.__hike_task, %struct.__hike_task* %task, i32 0, i32 1
  store i8* %env_ptr, i8** %p_env

  %need_buf = icmp sgt i32 %ret_size, 0
  br i1 %need_buf, label %alloc_buf, label %no_buf
alloc_buf:
  %buf = call i8* @malloc(i32 %ret_size)
  br label %set_buf
no_buf:
  br label %set_buf
set_buf:
  %buf_val = phi i8* [ %buf, %alloc_buf ], [ null, %no_buf ]
  %p_buf = getelementptr inbounds %struct.__hike_task, %struct.__hike_task* %task, i32 0, i32 2
  store i8* %buf_val, i8** %p_buf

  %p_done = getelementptr inbounds %struct.__hike_task, %struct.__hike_task* %task, i32 0, i32 3
  store i32 0, i32* %p_done

  ; POSIX/WASM 抽象イベントの作成
  %ev = call i8* @hike_event_create()
  %p_ev = getelementptr inbounds %struct.__hike_task, %struct.__hike_task* %task, i32 0, i32 4
  store i8* %ev, i8** %p_ev

  ; スレッド生成 API を呼び出し
  call i32 @hike_thread_spawn(void (i8*)* @__hike_task_worker_thunk, i8* %raw_task)
  ret %struct.__hike_task* %task
}

; タスクの同期待ち (<- 演算子の実体)
define internal i8* @__hike_task_wait(%struct.__hike_task* %task) {
entry:
  %task_null = icmp eq %struct.__hike_task* %task, null
  br i1 %task_null, label %ret_null, label %check_done
ret_null:
  ret i8* null
check_done:
  %p_done = getelementptr inbounds %struct.__hike_task, %struct.__hike_task* %task, i32 0, i32 3
  %done = load i32, i32* %p_done
  %is_done = icmp ne i32 %done, 0
  br i1 %is_done, label %get_res, label %wait_ev
wait_ev:
  %p_ev = getelementptr inbounds %struct.__hike_task, %struct.__hike_task* %task, i32 0, i32 4
  %ev = load i8*, i8** %p_ev
  ; -1 = INFINITE 待機
  call i32 @hike_event_wait(i8* %ev, i32 -1)
  call void @hike_event_destroy(i8* %ev)
  store i8* null, i8** %p_ev
  br label %get_res
get_res:
  %p_buf = getelementptr inbounds %struct.__hike_task, %struct.__hike_task* %task, i32 0, i32 2
  %buf = load i8*, i8** %p_buf
  ret i8* %buf
}


@str.1 = private unnamed_addr constant [48 x i8] c"Hike WebAssembly core initialized successfully.\00", align 1
@str.2 = private unnamed_addr constant [13 x i8] c"status-badge\00", align 1
@str.3 = private unnamed_addr constant [22 x i8] c"Running (Wasm Active)\00", align 1
@str.4 = private unnamed_addr constant [8 x i8] c"#10b981\00", align 1
@str.5 = private unnamed_addr constant [12 x i8] c"wasm-output\00", align 1
@str.6 = private unnamed_addr constant [94 x i8] c"Hike Language Wasm Runtime is online.\0AClick the buttons below to trigger native computations.\00", align 1
@str.7 = private unnamed_addr constant [47 x i8] c"Executing RunComputation inside WebAssembly...\00", align 1
@str.8 = private unnamed_addr constant [13 x i8] c"wasm-log-box\00", align 1
@str.9 = private unnamed_addr constant [62 x i8] c"\0A[Wasm Event] User clicked Action A: Memory layout validated.\00", align 1
@str.10 = private unnamed_addr constant [73 x i8] c"\0A[Wasm Event] User clicked Action B: Slice buffer manipulation complete.\00", align 1
@str.11 = private unnamed_addr constant [52 x i8] c"\0A[Wasm Event] Heartbeat tick received from browser.\00", align 1

declare void @js_log(i8*, i32)
declare void @js_set_text(i8*, i32, i8*, i32)
declare void @js_append_text(i8*, i32, i8*, i32)
declare void @js_set_badge_color(i8*, i32, i8*, i32)
define void @Log(i8* %msg_arg.1) {
entry:
  %msg.2 = alloca i8*
  store i8* %msg_arg.1, i8** %msg.2
  %v3 = load i8*, i8** %msg.2
  %v4 = load i8*, i8** %msg.2
  %v5 = call i32 @strlen32(i8* %v4)
  call void @js_log(i8* %v3, i32 %v5)
  ret void
}

define void @SetText(i8* %elementId_arg.1, i8* %text_arg.3) {
entry:
  %elementId.2 = alloca i8*
  store i8* %elementId_arg.1, i8** %elementId.2
  %text.4 = alloca i8*
  store i8* %text_arg.3, i8** %text.4
  %v5 = load i8*, i8** %elementId.2
  %v6 = load i8*, i8** %elementId.2
  %v7 = call i32 @strlen32(i8* %v6)
  %v8 = load i8*, i8** %text.4
  %v9 = load i8*, i8** %text.4
  %v10 = call i32 @strlen32(i8* %v9)
  call void @js_set_text(i8* %v5, i32 %v7, i8* %v8, i32 %v10)
  ret void
}

define void @AppendText(i8* %elementId_arg.1, i8* %text_arg.3) {
entry:
  %elementId.2 = alloca i8*
  store i8* %elementId_arg.1, i8** %elementId.2
  %text.4 = alloca i8*
  store i8* %text_arg.3, i8** %text.4
  %v5 = load i8*, i8** %elementId.2
  %v6 = load i8*, i8** %elementId.2
  %v7 = call i32 @strlen32(i8* %v6)
  %v8 = load i8*, i8** %text.4
  %v9 = load i8*, i8** %text.4
  %v10 = call i32 @strlen32(i8* %v9)
  call void @js_append_text(i8* %v5, i32 %v7, i8* %v8, i32 %v10)
  ret void
}

define void @SetBadgeColor(i8* %elementId_arg.1, i8* %color_arg.3) {
entry:
  %elementId.2 = alloca i8*
  store i8* %elementId_arg.1, i8** %elementId.2
  %color.4 = alloca i8*
  store i8* %color_arg.3, i8** %color.4
  %v5 = load i8*, i8** %elementId.2
  %v6 = load i8*, i8** %elementId.2
  %v7 = call i32 @strlen32(i8* %v6)
  %v8 = load i8*, i8** %color.4
  %v9 = load i8*, i8** %color.4
  %v10 = call i32 @strlen32(i8* %v9)
  call void @js_set_badge_color(i8* %v5, i32 %v7, i8* %v8, i32 %v10)
  ret void
}

define i32 @Fib(i32 %n_arg.1) {
entry:
  %n.2 = alloca i32
  store i32 %n_arg.1, i32* %n.2
  %v3 = load i32, i32* %n.2
  %v4 = icmp sle i32 %v3, 1
  br i1 %v4, label %if.then.1, label %if.end.3
if.then.1:
  %v5 = load i32, i32* %n.2
  ret i32 %v5
if.end.3:
  %v6 = load i32, i32* %n.2
  %v7 = sub i32 %v6, 1
  %v8 = call i32 @Fib(i32 %v7)
  %v9 = load i32, i32* %n.2
  %v10 = sub i32 %v9, 2
  %v11 = call i32 @Fib(i32 %v10)
  %v12 = add i32 %v8, %v11
  ret i32 %v12
}

define void @__hike_impl_InitApp() {
entry:
  %.b1 = getelementptr inbounds [48 x i8], [48 x i8]* @str.1, i32 0, i32 0
  call void @Log(i8* %.b1)
  %.b2 = getelementptr inbounds [13 x i8], [13 x i8]* @str.2, i32 0, i32 0
  %.b3 = getelementptr inbounds [22 x i8], [22 x i8]* @str.3, i32 0, i32 0
  call void @SetText(i8* %.b2, i8* %.b3)
  %.b4 = getelementptr inbounds [13 x i8], [13 x i8]* @str.2, i32 0, i32 0
  %.b5 = getelementptr inbounds [8 x i8], [8 x i8]* @str.4, i32 0, i32 0
  call void @SetBadgeColor(i8* %.b4, i8* %.b5)
  %.b6 = getelementptr inbounds [12 x i8], [12 x i8]* @str.5, i32 0, i32 0
  %.b7 = getelementptr inbounds [94 x i8], [94 x i8]* @str.6, i32 0, i32 0
  call void @SetText(i8* %.b6, i8* %.b7)
  ret void
}

define void @InitApp() {
entry:
  call void @__hike_impl_InitApp()
  ret void
}

define i32 @__hike_impl_AddNumbers(i32 %a_arg.1, i32 %b_arg.3) {
entry:
  %a.2 = alloca i32
  store i32 %a_arg.1, i32* %a.2
  %b.4 = alloca i32
  store i32 %b_arg.3, i32* %b.4
  %v5 = load i32, i32* %a.2
  %v6 = load i32, i32* %b.4
  %v7 = add i32 %v5, %v6
  ret i32 %v7
}

define i32 @AddNumbers(i32 %arg_0.1, i32 %arg_1.2) {
entry:
  %v3 = call i32 @__hike_impl_AddNumbers(i32 %arg_0.1, i32 %arg_1.2)
  ret i32 %v3
}

define i32 @__hike_impl_RunComputation(i32 %n_arg.1) {
entry:
  %n.2 = alloca i32
  store i32 %n_arg.1, i32* %n.2
  %.b8 = getelementptr inbounds [47 x i8], [47 x i8]* @str.7, i32 0, i32 0
  call void @Log(i8* %.b8)
  %v3 = load i32, i32* %n.2
  %v4 = call i32 @Fib(i32 %v3)
  %result.5 = alloca i32
  store i32 %v4, i32* %result.5
  %v6 = load i32, i32* %result.5
  ret i32 %v6
}

define i32 @RunComputation(i32 %arg_0.1) {
entry:
  %v2 = call i32 @__hike_impl_RunComputation(i32 %arg_0.1)
  ret i32 %v2
}

define void @__hike_impl_AppendLogMessage(i32 %msgCode_arg.1) {
entry:
  %msgCode.2 = alloca i32
  store i32 %msgCode_arg.1, i32* %msgCode.2
  %v3 = load i32, i32* %msgCode.2
  %v4 = icmp eq i32 %v3, 1
  br i1 %v4, label %if.then.4, label %if.else.5
if.then.4:
  %.b9 = getelementptr inbounds [13 x i8], [13 x i8]* @str.8, i32 0, i32 0
  %.b10 = getelementptr inbounds [62 x i8], [62 x i8]* @str.9, i32 0, i32 0
  call void @AppendText(i8* %.b9, i8* %.b10)
  br label %if.end.6
if.else.5:
  %v5 = load i32, i32* %msgCode.2
  %v6 = icmp eq i32 %v5, 2
  br i1 %v6, label %if.then.7, label %if.else.8
if.then.7:
  %.b11 = getelementptr inbounds [13 x i8], [13 x i8]* @str.8, i32 0, i32 0
  %.b12 = getelementptr inbounds [73 x i8], [73 x i8]* @str.10, i32 0, i32 0
  call void @AppendText(i8* %.b11, i8* %.b12)
  br label %if.end.9
if.else.8:
  %.b13 = getelementptr inbounds [13 x i8], [13 x i8]* @str.8, i32 0, i32 0
  %.b14 = getelementptr inbounds [52 x i8], [52 x i8]* @str.11, i32 0, i32 0
  call void @AppendText(i8* %.b13, i8* %.b14)
  br label %if.end.9
if.end.9:
  br label %if.end.6
if.end.6:
  ret void
}

define void @AppendLogMessage(i32 %arg_0.1) {
entry:
  call void @__hike_impl_AppendLogMessage(i32 %arg_0.1)
  ret void
}


; R0 compatibility restoration from pinned runtime.ll
define internal i32 @strlen32(i8* %s) #0 {
entry:
  %is_null = icmp eq i8* %s, null
  br i1 %is_null, label %ret_zero, label %loop.body
loop.body:
  %len = phi i32 [ 0, %entry ], [ %len.next, %loop.body ]
  %p = getelementptr inbounds i8, i8* %s, i32 %len
  %c = load i8, i8* %p, align 1
  %is_end = icmp eq i8 %c, 0
  %len.next = add i32 %len, 1
  br i1 %is_end, label %ret_len, label %loop.body
ret_len:
  ret i32 %len
ret_zero:
  ret i32 0
}
