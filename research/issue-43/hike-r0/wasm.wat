(module
  (type $t0 (func (param i32 i32)))
  (type $t1 (func (param i32 i32 i32 i32)))
  (type $t2 (func))
  (type $t3 (func (param i32)))
  (type $t4 (func (param i32) (result i32)))
  (type $t5 (func (param i32 i32) (result i32)))
  (import "env" "js_log" (func $env.js_log (type $t0)))
  (import "env" "js_set_text" (func $env.js_set_text (type $t1)))
  (import "env" "js_append_text" (func $env.js_append_text (type $t1)))
  (import "env" "js_set_badge_color" (func $env.js_set_badge_color (type $t1)))
  (func $__wasm_call_ctors (type $t2)
    nop)
  (func $Log (type $t3) (param $p0 i32)
    (local $l1 i32) (local $l2 i32)
    local.get $p0
    i32.eqz
    if $I0
      local.get $p0
      i32.const 0
      call $env.js_log
      return
    end
    loop $L1
      local.get $p0
      local.get $l1
      i32.add
      local.set $l2
      local.get $l1
      i32.const 1
      i32.add
      local.set $l1
      local.get $l2
      i32.load8_u
      br_if $L1
    end
    local.get $p0
    local.get $l1
    i32.const 1
    i32.sub
    call $env.js_log)
  (func $SetText (type $t0) (param $p0 i32) (param $p1 i32)
    (local $l2 i32) (local $l3 i32) (local $l4 i32)
    local.get $p0
    local.get $p0
    if $I0 (result i32)
      loop $L1
        local.get $p0
        local.get $l2
        i32.add
        local.set $l4
        local.get $l2
        i32.const 1
        i32.add
        local.set $l2
        local.get $l4
        i32.load8_u
        br_if $L1
      end
      local.get $l2
      i32.const 1
      i32.sub
    else
      local.get $l3
    end
    local.get $p1
    local.get $p1
    if $I2 (result i32)
      i32.const 0
      local.set $l2
      loop $L3
        local.get $p1
        local.get $l2
        i32.add
        local.set $l4
        local.get $l2
        i32.const 1
        i32.add
        local.set $l2
        local.get $l4
        i32.load8_u
        br_if $L3
      end
      local.get $l2
      i32.const 1
      i32.sub
    else
      local.get $l3
    end
    call $env.js_set_text)
  (func $AppendText (type $t0) (param $p0 i32) (param $p1 i32)
    (local $l2 i32) (local $l3 i32) (local $l4 i32)
    local.get $p0
    local.get $p0
    if $I0 (result i32)
      loop $L1
        local.get $p0
        local.get $l2
        i32.add
        local.set $l4
        local.get $l2
        i32.const 1
        i32.add
        local.set $l2
        local.get $l4
        i32.load8_u
        br_if $L1
      end
      local.get $l2
      i32.const 1
      i32.sub
    else
      local.get $l3
    end
    local.get $p1
    local.get $p1
    if $I2 (result i32)
      i32.const 0
      local.set $l2
      loop $L3
        local.get $p1
        local.get $l2
        i32.add
        local.set $l4
        local.get $l2
        i32.const 1
        i32.add
        local.set $l2
        local.get $l4
        i32.load8_u
        br_if $L3
      end
      local.get $l2
      i32.const 1
      i32.sub
    else
      local.get $l3
    end
    call $env.js_append_text)
  (func $SetBadgeColor (type $t0) (param $p0 i32) (param $p1 i32)
    (local $l2 i32) (local $l3 i32) (local $l4 i32)
    local.get $p0
    local.get $p0
    if $I0 (result i32)
      loop $L1
        local.get $p0
        local.get $l2
        i32.add
        local.set $l4
        local.get $l2
        i32.const 1
        i32.add
        local.set $l2
        local.get $l4
        i32.load8_u
        br_if $L1
      end
      local.get $l2
      i32.const 1
      i32.sub
    else
      local.get $l3
    end
    local.get $p1
    local.get $p1
    if $I2 (result i32)
      i32.const 0
      local.set $l2
      loop $L3
        local.get $p1
        local.get $l2
        i32.add
        local.set $l4
        local.get $l2
        i32.const 1
        i32.add
        local.set $l2
        local.get $l4
        i32.load8_u
        br_if $L3
      end
      local.get $l2
      i32.const 1
      i32.sub
    else
      local.get $l3
    end
    call $env.js_set_badge_color)
  (func $Fib (type $t4) (param $p0 i32) (result i32)
    (local $l1 i32) (local $l2 i32)
    local.get $p0
    i32.const 2
    i32.ge_s
    if $I0
      loop $L1
        local.get $p0
        i32.const 1
        i32.sub
        call $Fib
        local.get $l1
        i32.add
        local.set $l1
        local.get $p0
        i32.const 4
        i32.lt_u
        local.set $l2
        local.get $p0
        i32.const 2
        i32.sub
        local.set $p0
        local.get $l2
        i32.eqz
        br_if $L1
      end
    end
    local.get $p0
    local.get $l1
    i32.add)
  (func $__hike_impl_InitApp (type $t2)
    i32.const 1070
    i32.const 47
    call $env.js_log
    i32.const 1049
    i32.const 12
    i32.const 1446
    i32.const 21
    call $env.js_set_text
    i32.const 1049
    i32.const 12
    i32.const 1062
    i32.const 7
    call $env.js_set_badge_color
    i32.const 1037
    i32.const 11
    i32.const 1118
    i32.const 93
    call $env.js_set_text)
  (func $__hike_impl_AddNumbers (type $t5) (param $p0 i32) (param $p1 i32) (result i32)
    local.get $p0
    local.get $p1
    i32.add)
  (func $__hike_impl_RunComputation (type $t4) (param $p0 i32) (result i32)
    i32.const 1399
    i32.const 46
    call $env.js_log
    local.get $p0
    call $Fib)
  (func $__hike_impl_AppendLogMessage (type $t3) (param $p0 i32)
    block $B0
      block $B1
        block $B2
          local.get $p0
          i32.const 1
          i32.sub
          br_table $B1 $B2 $B0
        end
        i32.const 1024
        i32.const 12
        i32.const 1264
        i32.const 72
        call $env.js_append_text
        return
      end
      i32.const 1024
      i32.const 12
      i32.const 1337
      i32.const 61
      call $env.js_append_text
      return
    end
    i32.const 1024
    i32.const 12
    i32.const 1212
    i32.const 51
    call $env.js_append_text)
  (memory $memory 2)
  (global $__dso_handle i32 (i32.const 1024))
  (global $__data_end i32 (i32.const 1468))
  (global $__global_base i32 (i32.const 1024))
  (global $__heap_base i32 (i32.const 67008))
  (global $__memory_base i32 (i32.const 0))
  (global $__table_base i32 (i32.const 1))
  (export "memory" (memory $memory))
  (export "__wasm_call_ctors" (func $__wasm_call_ctors))
  (export "Log" (func $Log))
  (export "SetText" (func $SetText))
  (export "AppendText" (func $AppendText))
  (export "SetBadgeColor" (func $SetBadgeColor))
  (export "Fib" (func $Fib))
  (export "__hike_impl_InitApp" (func $__hike_impl_InitApp))
  (export "InitApp" (func $__hike_impl_InitApp))
  (export "__hike_impl_AddNumbers" (func $__hike_impl_AddNumbers))
  (export "AddNumbers" (func $__hike_impl_AddNumbers))
  (export "__hike_impl_RunComputation" (func $__hike_impl_RunComputation))
  (export "RunComputation" (func $__hike_impl_RunComputation))
  (export "__hike_impl_AppendLogMessage" (func $__hike_impl_AppendLogMessage))
  (export "AppendLogMessage" (func $__hike_impl_AppendLogMessage))
  (export "__dso_handle" (global $__dso_handle))
  (export "__data_end" (global $__data_end))
  (export "__global_base" (global $__global_base))
  (export "__heap_base" (global $__heap_base))
  (export "__memory_base" (global $__memory_base))
  (export "__table_base" (global $__table_base))
  (data $d0 (i32.const 1024) "wasm-log-box\00wasm-output\00status-badge\00#10b981\00Hike WebAssembly core initialized successfully.\00Hike Language Wasm Runtime is online.\0aClick the buttons below to trigger native computations.\00\0a[Wasm Event] Heartbeat tick received from browser.\00\0a[Wasm Event] User clicked Action B: Slice buffer manipulation complete.\00\0a[Wasm Event] User clicked Action A: Memory layout validated.\00Executing RunComputation inside WebAssembly...\00Running (Wasm Active)"))
