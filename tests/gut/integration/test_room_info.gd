extends GutTest

const TEST_HELPERS = preload("res://tests/gut/helpers/transport_test_helpers.gd")
const ROOM_URL := "wss://archipelago.gg:39841" #Private Archipelago room URL
const CONNECT_TIMEOUT_MSEC := 20000
const MESSAGE_TIMEOUT_MSEC := 5000

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

func test_room_sends_room_info_without_connect_packet() -> void:
	_transport = ClassDB.instantiate("YawcTransport") as Node
	assert_not_null(_transport, "transport should instantiate")
	if _transport == null:
		return
	_transport.opened.connect(_on_opened)
	_transport.closed.connect(_on_closed)
	_transport.error.connect(_on_error)
	_transport.text_message.connect(_on_text_message)
	add_child(_transport)

	assert_true(_transport.connect_to_url(ROOM_URL), "connection command should queue")
	var opened := await TEST_HELPERS.wait_until(
		get_tree(),
		func(): return _opened or not _error_message.is_empty(),
		CONNECT_TIMEOUT_MSEC,
	)
	assert_true(opened, "room WebSocket should open")
	assert_true(_error_message.is_empty(), "room connection should not emit an error")
	if not _opened or not _error_message.is_empty():
		return

	var received := await TEST_HELPERS.wait_until(
		get_tree(),
		func(): return not _received_text.is_empty() or not _error_message.is_empty(),
		MESSAGE_TIMEOUT_MSEC,
	)
	assert_true(received, "room should send an unsolicited message")
	assert_true(_error_message.is_empty(), "room should remain connected while sending RoomInfo")
	if not received or not _error_message.is_empty():
		return

	var messages = JSON.parse_string(_received_text)
	assert_true(messages is Array, "AP message should be a JSON array")
	if not messages is Array:
		return

	var room_info_found := false
	for message in messages:
		if message is Dictionary and message.get("cmd", "") == "RoomInfo":
			room_info_found = true
			break
	assert_true(room_info_found, "unsolicited AP messages should contain RoomInfo")
	assert_true(_signals_on_main_thread, "all signals should run on the main thread")
	if not room_info_found:
		return

	_transport.close()
	assert_true(
		await TEST_HELPERS.wait_until(get_tree(), func(): return _closed, MESSAGE_TIMEOUT_MSEC),
		"closed signal should arrive",
	)

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