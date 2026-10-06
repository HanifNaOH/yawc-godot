extends GutTest

const TEST_HELPERS = preload("res://tests/gut/helpers/transport_test_helpers.gd")
const TEST_URL := "ws://127.0.0.1:18765"

var _transport: Node
var _opened := false
var _closed := false
var _received_text := ""
var _error_message := ""
var _signals_on_main_thread := true

func before_each() -> void:
	_transport = null
	_opened = false
	_closed = false
	_received_text = ""
	_error_message = ""
	_signals_on_main_thread = true

func after_each() -> void:
	if is_instance_valid(_transport):
		_transport.free()

func test_connect_echo_close_and_main_thread_signals() -> void:
	_transport = ClassDB.instantiate("YawcTransport") as Node
	assert_not_null(_transport, "transport should instantiate")
	if _transport == null:
		return
	_transport.opened.connect(_on_opened)
	_transport.closed.connect(_on_closed)
	_transport.error.connect(_on_error)
	_transport.text_message.connect(_on_text_message)
	add_child(_transport)

	assert_true(_transport.connect_to_url(TEST_URL), "connection command should queue")
	var opened := await TEST_HELPERS.wait_until(
		get_tree(),
		func(): return _opened or not _error_message.is_empty(),
		10000,
	)
	assert_true(opened, "connection should open")
	assert_true(_error_message.is_empty(), "connection should not emit an error")
	if not _opened or not _error_message.is_empty():
		return

	assert_true(_transport.send_text("bridge-test"), "text command should queue")
	var received := await TEST_HELPERS.wait_until(
		get_tree(),
		func(): return not _received_text.is_empty() or not _error_message.is_empty(),
		5000,
	)
	assert_true(received, "echo response should arrive")
	assert_eq(_received_text, "echo:bridge-test")
	if not _error_message.is_empty():
		return

	_transport.close()
	assert_true(
		await TEST_HELPERS.wait_until(get_tree(), func(): return _closed, 5000),
		"closed signal should arrive",
	)
	assert_true(_signals_on_main_thread, "all signals should run on the main thread")

func _record_signal_thread() -> void:
	if not Thread.is_main_thread():
		_signals_on_main_thread = false

func _on_opened() -> void:
	_record_signal_thread()
	_opened = true

func _on_closed() -> void:
	_record_signal_thread()
	_closed = true

func _on_error(message: String) -> void:
	_record_signal_thread()
	_error_message = message

func _on_text_message(message: String) -> void:
	_record_signal_thread()
	_received_text = message