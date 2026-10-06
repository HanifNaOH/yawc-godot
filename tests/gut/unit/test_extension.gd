extends GutTest

func test_extension_loads_and_registers_transport_node() -> void:
	var extension = load("res://addons/yawc_transport/yawc_godot.gdextension")
	assert_not_null(extension, "GDExtension descriptor should load")
	assert_true(ClassDB.class_exists("YawcTransport"), "YawcTransport should be registered")

	var transport = ClassDB.instantiate("YawcTransport") as Node
	assert_not_null(transport, "YawcTransport should instantiate")
	if transport != null:
		transport.free()