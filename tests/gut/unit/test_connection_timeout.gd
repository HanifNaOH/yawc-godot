extends GutTest

const TEST_HELPERS = preload("res://tests/gut/helpers/transport_test_helpers.gd")
const STALLED_PORT := 18766
const CONNECTION_TEST_TIMEOUT_MSEC := 20000
const SOCKET_CLOSE_TIMEOUT_MSEC := 2000

var _transport: Node
var _server: TCPServer
var _peer: StreamPeerTCP
var _error_message := ""
var _connection_closed := false

func before_each() -> void:
	_transport = null
	_server = null
	_peer = null
	_error_message = ""
	_connection_closed = false

func after_each() -> void:
	if is_instance_valid(_transport):
		_transport.free()
	if is_instance_valid(_peer):
		_peer.disconnect_from_host()
	if is_instance_valid(_server):
		_server.stop()

func test_stalled_handshake_times_out_and_closes_worker() -> void:
	_server = TCPServer.new()
	assert_eq(_server.listen(STALLED_PORT, "127.0.0.1"), OK, "stalled test server should listen")
	if not _server.is_listening():
		return

	_transport = ClassDB.instantiate("YawcTransport") as Node
	assert_not_null(_transport, "transport should instantiate")
	if _transport == null:
		return
	_transport.error.connect(_on_error)
	_transport.closed.connect(_on_closed)
	add_child(_transport)

	assert_true(
		_transport.connect_to_url("ws://127.0.0.1:%d/" % STALLED_PORT),
		"connection command should queue",
	)
	var accept_deadline := Time.get_ticks_msec() + 5000
	while _peer == null and Time.get_ticks_msec() < accept_deadline:
		if _server.is_connection_available():
			_peer = _server.take_connection()
		await get_tree().process_frame
	assert_not_null(_peer, "worker should reach the stalled TCP server")
	if _peer == null:
		return

	var timed_out := await TEST_HELPERS.wait_until(
		get_tree(),
		func(): return not _error_message.is_empty() and _connection_closed,
		CONNECTION_TEST_TIMEOUT_MSEC,
	)
	assert_true(timed_out, "handshake should report timeout and closed state")
	assert_true(_error_message.contains("timed out"), "error should identify timeout")
	if not timed_out:
		return

	var peer_disconnected := await TEST_HELPERS.wait_until(
		get_tree(),
		func(): return _is_peer_disconnected(),
		SOCKET_CLOSE_TIMEOUT_MSEC,
	)
	assert_true(peer_disconnected, "timed-out client socket should disconnect")

func test_invalid_url_is_rejected() -> void:
	_transport = ClassDB.instantiate("YawcTransport") as Node
	assert_not_null(_transport, "transport should instantiate")
	if _transport == null:
		return
	_transport.error.connect(_on_error)
	_transport.closed.connect(_on_closed)
	add_child(_transport)

	assert_true(_transport.connect_to_url("not a websocket url"), "command should queue")
	var rejected := await TEST_HELPERS.wait_until(
		get_tree(),
		func(): return not _error_message.is_empty() and _connection_closed,
		5000,
	)
	assert_true(rejected, "invalid URL should emit error and closed")
	assert_false(_error_message.is_empty(), "invalid URL should provide an error")

func _is_peer_disconnected() -> bool:
	_peer.poll()
	if _peer.get_status() == StreamPeerTCP.STATUS_CONNECTED:
		var available_bytes = _peer.get_available_bytes()
		if available_bytes > 0:
			_peer.get_data(available_bytes)
	return _peer.get_status() != StreamPeerTCP.STATUS_CONNECTED

func _on_error(message: String) -> void:
	_error_message = message

func _on_closed() -> void:
	_connection_closed = true