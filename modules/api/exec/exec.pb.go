// Code generated by protoc-gen-go. DO NOT EDIT.
// versions:
// 	protoc-gen-go v1.36.1
// 	protoc        v5.29.1
// source: exec/exec.proto

package exec

import (
	protoreflect "google.golang.org/protobuf/reflect/protoreflect"
	protoimpl "google.golang.org/protobuf/runtime/protoimpl"
	reflect "reflect"
	sync "sync"
)

const (
	// Verify that this generated code is sufficiently up-to-date.
	_ = protoimpl.EnforceVersion(20 - protoimpl.MinVersion)
	// Verify that runtime/protoimpl is sufficiently up-to-date.
	_ = protoimpl.EnforceVersion(protoimpl.MaxVersion - 20)
)

// Define the request to execute a subprocess command
type StartCommand struct {
	state            protoimpl.MessageState `protogen:"open.v1"`
	Command          string                 `protobuf:"bytes,1,opt,name=command,proto3" json:"command,omitempty"`                                                                                          // The shell command to execute
	Arguments        []string               `protobuf:"bytes,2,rep,name=arguments,proto3" json:"arguments,omitempty"`                                                                                      // Arguments for the command
	WorkingDirectory *string                `protobuf:"bytes,3,opt,name=working_directory,json=workingDirectory,proto3,oneof" json:"working_directory,omitempty"`                                          // (Optional) Directory to execute the command in
	EnvVars          map[string]string      `protobuf:"bytes,4,rep,name=env_vars,json=envVars,proto3" json:"env_vars,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"` // (Optional) Environment variables
	Stdin            []byte                 `protobuf:"bytes,5,opt,name=stdin,proto3,oneof" json:"stdin,omitempty"`                                                                                        // Initial stdin sequence
	Role             *string                `protobuf:"bytes,6,opt,name=role,proto3,oneof" json:"role,omitempty"`                                                                                          // Role is actually a user name or security token
	unknownFields    protoimpl.UnknownFields
	sizeCache        protoimpl.SizeCache
}

func (x *StartCommand) Reset() {
	*x = StartCommand{}
	mi := &file_exec_exec_proto_msgTypes[0]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *StartCommand) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*StartCommand) ProtoMessage() {}

func (x *StartCommand) ProtoReflect() protoreflect.Message {
	mi := &file_exec_exec_proto_msgTypes[0]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use StartCommand.ProtoReflect.Descriptor instead.
func (*StartCommand) Descriptor() ([]byte, []int) {
	return file_exec_exec_proto_rawDescGZIP(), []int{0}
}

func (x *StartCommand) GetCommand() string {
	if x != nil {
		return x.Command
	}
	return ""
}

func (x *StartCommand) GetArguments() []string {
	if x != nil {
		return x.Arguments
	}
	return nil
}

func (x *StartCommand) GetWorkingDirectory() string {
	if x != nil && x.WorkingDirectory != nil {
		return *x.WorkingDirectory
	}
	return ""
}

func (x *StartCommand) GetEnvVars() map[string]string {
	if x != nil {
		return x.EnvVars
	}
	return nil
}

func (x *StartCommand) GetStdin() []byte {
	if x != nil {
		return x.Stdin
	}
	return nil
}

func (x *StartCommand) GetRole() string {
	if x != nil && x.Role != nil {
		return *x.Role
	}
	return ""
}

type CommandIO struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	Payload       []byte                 `protobuf:"bytes,1,opt,name=payload,proto3" json:"payload,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *CommandIO) Reset() {
	*x = CommandIO{}
	mi := &file_exec_exec_proto_msgTypes[1]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *CommandIO) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*CommandIO) ProtoMessage() {}

func (x *CommandIO) ProtoReflect() protoreflect.Message {
	mi := &file_exec_exec_proto_msgTypes[1]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use CommandIO.ProtoReflect.Descriptor instead.
func (*CommandIO) Descriptor() ([]byte, []int) {
	return file_exec_exec_proto_rawDescGZIP(), []int{1}
}

func (x *CommandIO) GetPayload() []byte {
	if x != nil {
		return x.Payload
	}
	return nil
}

type SignalCommand struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	Signal        int32                  `protobuf:"varint,1,opt,name=signal,proto3" json:"signal,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *SignalCommand) Reset() {
	*x = SignalCommand{}
	mi := &file_exec_exec_proto_msgTypes[2]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *SignalCommand) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*SignalCommand) ProtoMessage() {}

func (x *SignalCommand) ProtoReflect() protoreflect.Message {
	mi := &file_exec_exec_proto_msgTypes[2]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use SignalCommand.ProtoReflect.Descriptor instead.
func (*SignalCommand) Descriptor() ([]byte, []int) {
	return file_exec_exec_proto_rawDescGZIP(), []int{2}
}

func (x *SignalCommand) GetSignal() int32 {
	if x != nil {
		return x.Signal
	}
	return 0
}

type StartedEvent struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	Pid           int32                  `protobuf:"varint,1,opt,name=pid,proto3" json:"pid,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *StartedEvent) Reset() {
	*x = StartedEvent{}
	mi := &file_exec_exec_proto_msgTypes[3]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *StartedEvent) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*StartedEvent) ProtoMessage() {}

func (x *StartedEvent) ProtoReflect() protoreflect.Message {
	mi := &file_exec_exec_proto_msgTypes[3]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use StartedEvent.ProtoReflect.Descriptor instead.
func (*StartedEvent) Descriptor() ([]byte, []int) {
	return file_exec_exec_proto_rawDescGZIP(), []int{3}
}

func (x *StartedEvent) GetPid() int32 {
	if x != nil {
		return x.Pid
	}
	return 0
}

type FinishedEvent struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	ReturnCode    int32                  `protobuf:"varint,1,opt,name=return_code,json=returnCode,proto3" json:"return_code,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *FinishedEvent) Reset() {
	*x = FinishedEvent{}
	mi := &file_exec_exec_proto_msgTypes[4]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *FinishedEvent) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*FinishedEvent) ProtoMessage() {}

func (x *FinishedEvent) ProtoReflect() protoreflect.Message {
	mi := &file_exec_exec_proto_msgTypes[4]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use FinishedEvent.ProtoReflect.Descriptor instead.
func (*FinishedEvent) Descriptor() ([]byte, []int) {
	return file_exec_exec_proto_rawDescGZIP(), []int{4}
}

func (x *FinishedEvent) GetReturnCode() int32 {
	if x != nil {
		return x.ReturnCode
	}
	return 0
}

type CommandRequest struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Types that are valid to be assigned to Command:
	//
	//	*CommandRequest_Start
	//	*CommandRequest_Stdin
	//	*CommandRequest_Signal
	Command       isCommandRequest_Command `protobuf_oneof:"Command"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *CommandRequest) Reset() {
	*x = CommandRequest{}
	mi := &file_exec_exec_proto_msgTypes[5]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *CommandRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*CommandRequest) ProtoMessage() {}

func (x *CommandRequest) ProtoReflect() protoreflect.Message {
	mi := &file_exec_exec_proto_msgTypes[5]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use CommandRequest.ProtoReflect.Descriptor instead.
func (*CommandRequest) Descriptor() ([]byte, []int) {
	return file_exec_exec_proto_rawDescGZIP(), []int{5}
}

func (x *CommandRequest) GetCommand() isCommandRequest_Command {
	if x != nil {
		return x.Command
	}
	return nil
}

func (x *CommandRequest) GetStart() *StartCommand {
	if x != nil {
		if x, ok := x.Command.(*CommandRequest_Start); ok {
			return x.Start
		}
	}
	return nil
}

func (x *CommandRequest) GetStdin() *CommandIO {
	if x != nil {
		if x, ok := x.Command.(*CommandRequest_Stdin); ok {
			return x.Stdin
		}
	}
	return nil
}

func (x *CommandRequest) GetSignal() *SignalCommand {
	if x != nil {
		if x, ok := x.Command.(*CommandRequest_Signal); ok {
			return x.Signal
		}
	}
	return nil
}

type isCommandRequest_Command interface {
	isCommandRequest_Command()
}

type CommandRequest_Start struct {
	Start *StartCommand `protobuf:"bytes,1,opt,name=Start,proto3,oneof"`
}

type CommandRequest_Stdin struct {
	Stdin *CommandIO `protobuf:"bytes,2,opt,name=Stdin,proto3,oneof"`
}

type CommandRequest_Signal struct {
	Signal *SignalCommand `protobuf:"bytes,3,opt,name=Signal,proto3,oneof"`
}

func (*CommandRequest_Start) isCommandRequest_Command() {}

func (*CommandRequest_Stdin) isCommandRequest_Command() {}

func (*CommandRequest_Signal) isCommandRequest_Command() {}

type CommandResponse struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Types that are valid to be assigned to Event:
	//
	//	*CommandResponse_Stdout
	//	*CommandResponse_Stderr
	//	*CommandResponse_Started
	//	*CommandResponse_Finished
	Event         isCommandResponse_Event `protobuf_oneof:"Event"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *CommandResponse) Reset() {
	*x = CommandResponse{}
	mi := &file_exec_exec_proto_msgTypes[6]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *CommandResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*CommandResponse) ProtoMessage() {}

func (x *CommandResponse) ProtoReflect() protoreflect.Message {
	mi := &file_exec_exec_proto_msgTypes[6]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use CommandResponse.ProtoReflect.Descriptor instead.
func (*CommandResponse) Descriptor() ([]byte, []int) {
	return file_exec_exec_proto_rawDescGZIP(), []int{6}
}

func (x *CommandResponse) GetEvent() isCommandResponse_Event {
	if x != nil {
		return x.Event
	}
	return nil
}

func (x *CommandResponse) GetStdout() *CommandIO {
	if x != nil {
		if x, ok := x.Event.(*CommandResponse_Stdout); ok {
			return x.Stdout
		}
	}
	return nil
}

func (x *CommandResponse) GetStderr() *CommandIO {
	if x != nil {
		if x, ok := x.Event.(*CommandResponse_Stderr); ok {
			return x.Stderr
		}
	}
	return nil
}

func (x *CommandResponse) GetStarted() *StartedEvent {
	if x != nil {
		if x, ok := x.Event.(*CommandResponse_Started); ok {
			return x.Started
		}
	}
	return nil
}

func (x *CommandResponse) GetFinished() *FinishedEvent {
	if x != nil {
		if x, ok := x.Event.(*CommandResponse_Finished); ok {
			return x.Finished
		}
	}
	return nil
}

type isCommandResponse_Event interface {
	isCommandResponse_Event()
}

type CommandResponse_Stdout struct {
	Stdout *CommandIO `protobuf:"bytes,1,opt,name=Stdout,proto3,oneof"`
}

type CommandResponse_Stderr struct {
	Stderr *CommandIO `protobuf:"bytes,2,opt,name=Stderr,proto3,oneof"`
}

type CommandResponse_Started struct {
	Started *StartedEvent `protobuf:"bytes,3,opt,name=Started,proto3,oneof"`
}

type CommandResponse_Finished struct {
	Finished *FinishedEvent `protobuf:"bytes,4,opt,name=Finished,proto3,oneof"`
}

func (*CommandResponse_Stdout) isCommandResponse_Event() {}

func (*CommandResponse_Stderr) isCommandResponse_Event() {}

func (*CommandResponse_Started) isCommandResponse_Event() {}

func (*CommandResponse_Finished) isCommandResponse_Event() {}

var File_exec_exec_proto protoreflect.FileDescriptor

var file_exec_exec_proto_rawDesc = []byte{
	0x0a, 0x0f, 0x65, 0x78, 0x65, 0x63, 0x2f, 0x65, 0x78, 0x65, 0x63, 0x2e, 0x70, 0x72, 0x6f, 0x74,
	0x6f, 0x12, 0x04, 0x65, 0x78, 0x65, 0x63, 0x22, 0xcd, 0x02, 0x0a, 0x0c, 0x53, 0x74, 0x61, 0x72,
	0x74, 0x43, 0x6f, 0x6d, 0x6d, 0x61, 0x6e, 0x64, 0x12, 0x18, 0x0a, 0x07, 0x63, 0x6f, 0x6d, 0x6d,
	0x61, 0x6e, 0x64, 0x18, 0x01, 0x20, 0x01, 0x28, 0x09, 0x52, 0x07, 0x63, 0x6f, 0x6d, 0x6d, 0x61,
	0x6e, 0x64, 0x12, 0x1c, 0x0a, 0x09, 0x61, 0x72, 0x67, 0x75, 0x6d, 0x65, 0x6e, 0x74, 0x73, 0x18,
	0x02, 0x20, 0x03, 0x28, 0x09, 0x52, 0x09, 0x61, 0x72, 0x67, 0x75, 0x6d, 0x65, 0x6e, 0x74, 0x73,
	0x12, 0x30, 0x0a, 0x11, 0x77, 0x6f, 0x72, 0x6b, 0x69, 0x6e, 0x67, 0x5f, 0x64, 0x69, 0x72, 0x65,
	0x63, 0x74, 0x6f, 0x72, 0x79, 0x18, 0x03, 0x20, 0x01, 0x28, 0x09, 0x48, 0x00, 0x52, 0x10, 0x77,
	0x6f, 0x72, 0x6b, 0x69, 0x6e, 0x67, 0x44, 0x69, 0x72, 0x65, 0x63, 0x74, 0x6f, 0x72, 0x79, 0x88,
	0x01, 0x01, 0x12, 0x3a, 0x0a, 0x08, 0x65, 0x6e, 0x76, 0x5f, 0x76, 0x61, 0x72, 0x73, 0x18, 0x04,
	0x20, 0x03, 0x28, 0x0b, 0x32, 0x1f, 0x2e, 0x65, 0x78, 0x65, 0x63, 0x2e, 0x53, 0x74, 0x61, 0x72,
	0x74, 0x43, 0x6f, 0x6d, 0x6d, 0x61, 0x6e, 0x64, 0x2e, 0x45, 0x6e, 0x76, 0x56, 0x61, 0x72, 0x73,
	0x45, 0x6e, 0x74, 0x72, 0x79, 0x52, 0x07, 0x65, 0x6e, 0x76, 0x56, 0x61, 0x72, 0x73, 0x12, 0x19,
	0x0a, 0x05, 0x73, 0x74, 0x64, 0x69, 0x6e, 0x18, 0x05, 0x20, 0x01, 0x28, 0x0c, 0x48, 0x01, 0x52,
	0x05, 0x73, 0x74, 0x64, 0x69, 0x6e, 0x88, 0x01, 0x01, 0x12, 0x17, 0x0a, 0x04, 0x72, 0x6f, 0x6c,
	0x65, 0x18, 0x06, 0x20, 0x01, 0x28, 0x09, 0x48, 0x02, 0x52, 0x04, 0x72, 0x6f, 0x6c, 0x65, 0x88,
	0x01, 0x01, 0x1a, 0x3a, 0x0a, 0x0c, 0x45, 0x6e, 0x76, 0x56, 0x61, 0x72, 0x73, 0x45, 0x6e, 0x74,
	0x72, 0x79, 0x12, 0x10, 0x0a, 0x03, 0x6b, 0x65, 0x79, 0x18, 0x01, 0x20, 0x01, 0x28, 0x09, 0x52,
	0x03, 0x6b, 0x65, 0x79, 0x12, 0x14, 0x0a, 0x05, 0x76, 0x61, 0x6c, 0x75, 0x65, 0x18, 0x02, 0x20,
	0x01, 0x28, 0x09, 0x52, 0x05, 0x76, 0x61, 0x6c, 0x75, 0x65, 0x3a, 0x02, 0x38, 0x01, 0x42, 0x14,
	0x0a, 0x12, 0x5f, 0x77, 0x6f, 0x72, 0x6b, 0x69, 0x6e, 0x67, 0x5f, 0x64, 0x69, 0x72, 0x65, 0x63,
	0x74, 0x6f, 0x72, 0x79, 0x42, 0x08, 0x0a, 0x06, 0x5f, 0x73, 0x74, 0x64, 0x69, 0x6e, 0x42, 0x07,
	0x0a, 0x05, 0x5f, 0x72, 0x6f, 0x6c, 0x65, 0x22, 0x25, 0x0a, 0x09, 0x43, 0x6f, 0x6d, 0x6d, 0x61,
	0x6e, 0x64, 0x49, 0x4f, 0x12, 0x18, 0x0a, 0x07, 0x70, 0x61, 0x79, 0x6c, 0x6f, 0x61, 0x64, 0x18,
	0x01, 0x20, 0x01, 0x28, 0x0c, 0x52, 0x07, 0x70, 0x61, 0x79, 0x6c, 0x6f, 0x61, 0x64, 0x22, 0x27,
	0x0a, 0x0d, 0x53, 0x69, 0x67, 0x6e, 0x61, 0x6c, 0x43, 0x6f, 0x6d, 0x6d, 0x61, 0x6e, 0x64, 0x12,
	0x16, 0x0a, 0x06, 0x73, 0x69, 0x67, 0x6e, 0x61, 0x6c, 0x18, 0x01, 0x20, 0x01, 0x28, 0x05, 0x52,
	0x06, 0x73, 0x69, 0x67, 0x6e, 0x61, 0x6c, 0x22, 0x20, 0x0a, 0x0c, 0x53, 0x74, 0x61, 0x72, 0x74,
	0x65, 0x64, 0x45, 0x76, 0x65, 0x6e, 0x74, 0x12, 0x10, 0x0a, 0x03, 0x70, 0x69, 0x64, 0x18, 0x01,
	0x20, 0x01, 0x28, 0x05, 0x52, 0x03, 0x70, 0x69, 0x64, 0x22, 0x30, 0x0a, 0x0d, 0x46, 0x69, 0x6e,
	0x69, 0x73, 0x68, 0x65, 0x64, 0x45, 0x76, 0x65, 0x6e, 0x74, 0x12, 0x1f, 0x0a, 0x0b, 0x72, 0x65,
	0x74, 0x75, 0x72, 0x6e, 0x5f, 0x63, 0x6f, 0x64, 0x65, 0x18, 0x01, 0x20, 0x01, 0x28, 0x05, 0x52,
	0x0a, 0x72, 0x65, 0x74, 0x75, 0x72, 0x6e, 0x43, 0x6f, 0x64, 0x65, 0x22, 0x9f, 0x01, 0x0a, 0x0e,
	0x43, 0x6f, 0x6d, 0x6d, 0x61, 0x6e, 0x64, 0x52, 0x65, 0x71, 0x75, 0x65, 0x73, 0x74, 0x12, 0x2a,
	0x0a, 0x05, 0x53, 0x74, 0x61, 0x72, 0x74, 0x18, 0x01, 0x20, 0x01, 0x28, 0x0b, 0x32, 0x12, 0x2e,
	0x65, 0x78, 0x65, 0x63, 0x2e, 0x53, 0x74, 0x61, 0x72, 0x74, 0x43, 0x6f, 0x6d, 0x6d, 0x61, 0x6e,
	0x64, 0x48, 0x00, 0x52, 0x05, 0x53, 0x74, 0x61, 0x72, 0x74, 0x12, 0x27, 0x0a, 0x05, 0x53, 0x74,
	0x64, 0x69, 0x6e, 0x18, 0x02, 0x20, 0x01, 0x28, 0x0b, 0x32, 0x0f, 0x2e, 0x65, 0x78, 0x65, 0x63,
	0x2e, 0x43, 0x6f, 0x6d, 0x6d, 0x61, 0x6e, 0x64, 0x49, 0x4f, 0x48, 0x00, 0x52, 0x05, 0x53, 0x74,
	0x64, 0x69, 0x6e, 0x12, 0x2d, 0x0a, 0x06, 0x53, 0x69, 0x67, 0x6e, 0x61, 0x6c, 0x18, 0x03, 0x20,
	0x01, 0x28, 0x0b, 0x32, 0x13, 0x2e, 0x65, 0x78, 0x65, 0x63, 0x2e, 0x53, 0x69, 0x67, 0x6e, 0x61,
	0x6c, 0x43, 0x6f, 0x6d, 0x6d, 0x61, 0x6e, 0x64, 0x48, 0x00, 0x52, 0x06, 0x53, 0x69, 0x67, 0x6e,
	0x61, 0x6c, 0x42, 0x09, 0x0a, 0x07, 0x43, 0x6f, 0x6d, 0x6d, 0x61, 0x6e, 0x64, 0x22, 0xd3, 0x01,
	0x0a, 0x0f, 0x43, 0x6f, 0x6d, 0x6d, 0x61, 0x6e, 0x64, 0x52, 0x65, 0x73, 0x70, 0x6f, 0x6e, 0x73,
	0x65, 0x12, 0x29, 0x0a, 0x06, 0x53, 0x74, 0x64, 0x6f, 0x75, 0x74, 0x18, 0x01, 0x20, 0x01, 0x28,
	0x0b, 0x32, 0x0f, 0x2e, 0x65, 0x78, 0x65, 0x63, 0x2e, 0x43, 0x6f, 0x6d, 0x6d, 0x61, 0x6e, 0x64,
	0x49, 0x4f, 0x48, 0x00, 0x52, 0x06, 0x53, 0x74, 0x64, 0x6f, 0x75, 0x74, 0x12, 0x29, 0x0a, 0x06,
	0x53, 0x74, 0x64, 0x65, 0x72, 0x72, 0x18, 0x02, 0x20, 0x01, 0x28, 0x0b, 0x32, 0x0f, 0x2e, 0x65,
	0x78, 0x65, 0x63, 0x2e, 0x43, 0x6f, 0x6d, 0x6d, 0x61, 0x6e, 0x64, 0x49, 0x4f, 0x48, 0x00, 0x52,
	0x06, 0x53, 0x74, 0x64, 0x65, 0x72, 0x72, 0x12, 0x2e, 0x0a, 0x07, 0x53, 0x74, 0x61, 0x72, 0x74,
	0x65, 0x64, 0x18, 0x03, 0x20, 0x01, 0x28, 0x0b, 0x32, 0x12, 0x2e, 0x65, 0x78, 0x65, 0x63, 0x2e,
	0x53, 0x74, 0x61, 0x72, 0x74, 0x65, 0x64, 0x45, 0x76, 0x65, 0x6e, 0x74, 0x48, 0x00, 0x52, 0x07,
	0x53, 0x74, 0x61, 0x72, 0x74, 0x65, 0x64, 0x12, 0x31, 0x0a, 0x08, 0x46, 0x69, 0x6e, 0x69, 0x73,
	0x68, 0x65, 0x64, 0x18, 0x04, 0x20, 0x01, 0x28, 0x0b, 0x32, 0x13, 0x2e, 0x65, 0x78, 0x65, 0x63,
	0x2e, 0x46, 0x69, 0x6e, 0x69, 0x73, 0x68, 0x65, 0x64, 0x45, 0x76, 0x65, 0x6e, 0x74, 0x48, 0x00,
	0x52, 0x08, 0x46, 0x69, 0x6e, 0x69, 0x73, 0x68, 0x65, 0x64, 0x42, 0x07, 0x0a, 0x05, 0x45, 0x76,
	0x65, 0x6e, 0x74, 0x32, 0x45, 0x0a, 0x04, 0x45, 0x78, 0x65, 0x63, 0x12, 0x3d, 0x0a, 0x0a, 0x52,
	0x75, 0x6e, 0x43, 0x6f, 0x6d, 0x6d, 0x61, 0x6e, 0x64, 0x12, 0x14, 0x2e, 0x65, 0x78, 0x65, 0x63,
	0x2e, 0x43, 0x6f, 0x6d, 0x6d, 0x61, 0x6e, 0x64, 0x52, 0x65, 0x71, 0x75, 0x65, 0x73, 0x74, 0x1a,
	0x15, 0x2e, 0x65, 0x78, 0x65, 0x63, 0x2e, 0x43, 0x6f, 0x6d, 0x6d, 0x61, 0x6e, 0x64, 0x52, 0x65,
	0x73, 0x70, 0x6f, 0x6e, 0x73, 0x65, 0x28, 0x01, 0x30, 0x01, 0x42, 0x08, 0x5a, 0x06, 0x2e, 0x2f,
	0x65, 0x78, 0x65, 0x63, 0x62, 0x06, 0x70, 0x72, 0x6f, 0x74, 0x6f, 0x33,
}

var (
	file_exec_exec_proto_rawDescOnce sync.Once
	file_exec_exec_proto_rawDescData = file_exec_exec_proto_rawDesc
)

func file_exec_exec_proto_rawDescGZIP() []byte {
	file_exec_exec_proto_rawDescOnce.Do(func() {
		file_exec_exec_proto_rawDescData = protoimpl.X.CompressGZIP(file_exec_exec_proto_rawDescData)
	})
	return file_exec_exec_proto_rawDescData
}

var file_exec_exec_proto_msgTypes = make([]protoimpl.MessageInfo, 8)
var file_exec_exec_proto_goTypes = []any{
	(*StartCommand)(nil),    // 0: exec.StartCommand
	(*CommandIO)(nil),       // 1: exec.CommandIO
	(*SignalCommand)(nil),   // 2: exec.SignalCommand
	(*StartedEvent)(nil),    // 3: exec.StartedEvent
	(*FinishedEvent)(nil),   // 4: exec.FinishedEvent
	(*CommandRequest)(nil),  // 5: exec.CommandRequest
	(*CommandResponse)(nil), // 6: exec.CommandResponse
	nil,                     // 7: exec.StartCommand.EnvVarsEntry
}
var file_exec_exec_proto_depIdxs = []int32{
	7, // 0: exec.StartCommand.env_vars:type_name -> exec.StartCommand.EnvVarsEntry
	0, // 1: exec.CommandRequest.Start:type_name -> exec.StartCommand
	1, // 2: exec.CommandRequest.Stdin:type_name -> exec.CommandIO
	2, // 3: exec.CommandRequest.Signal:type_name -> exec.SignalCommand
	1, // 4: exec.CommandResponse.Stdout:type_name -> exec.CommandIO
	1, // 5: exec.CommandResponse.Stderr:type_name -> exec.CommandIO
	3, // 6: exec.CommandResponse.Started:type_name -> exec.StartedEvent
	4, // 7: exec.CommandResponse.Finished:type_name -> exec.FinishedEvent
	5, // 8: exec.Exec.RunCommand:input_type -> exec.CommandRequest
	6, // 9: exec.Exec.RunCommand:output_type -> exec.CommandResponse
	9, // [9:10] is the sub-list for method output_type
	8, // [8:9] is the sub-list for method input_type
	8, // [8:8] is the sub-list for extension type_name
	8, // [8:8] is the sub-list for extension extendee
	0, // [0:8] is the sub-list for field type_name
}

func init() { file_exec_exec_proto_init() }
func file_exec_exec_proto_init() {
	if File_exec_exec_proto != nil {
		return
	}
	file_exec_exec_proto_msgTypes[0].OneofWrappers = []any{}
	file_exec_exec_proto_msgTypes[5].OneofWrappers = []any{
		(*CommandRequest_Start)(nil),
		(*CommandRequest_Stdin)(nil),
		(*CommandRequest_Signal)(nil),
	}
	file_exec_exec_proto_msgTypes[6].OneofWrappers = []any{
		(*CommandResponse_Stdout)(nil),
		(*CommandResponse_Stderr)(nil),
		(*CommandResponse_Started)(nil),
		(*CommandResponse_Finished)(nil),
	}
	type x struct{}
	out := protoimpl.TypeBuilder{
		File: protoimpl.DescBuilder{
			GoPackagePath: reflect.TypeOf(x{}).PkgPath(),
			RawDescriptor: file_exec_exec_proto_rawDesc,
			NumEnums:      0,
			NumMessages:   8,
			NumExtensions: 0,
			NumServices:   1,
		},
		GoTypes:           file_exec_exec_proto_goTypes,
		DependencyIndexes: file_exec_exec_proto_depIdxs,
		MessageInfos:      file_exec_exec_proto_msgTypes,
	}.Build()
	File_exec_exec_proto = out.File
	file_exec_exec_proto_rawDesc = nil
	file_exec_exec_proto_goTypes = nil
	file_exec_exec_proto_depIdxs = nil
}
