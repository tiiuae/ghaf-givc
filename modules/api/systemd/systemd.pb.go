// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

// Code generated by protoc-gen-go. DO NOT EDIT.
// versions:
// 	protoc-gen-go v1.36.6
// 	protoc        v5.29.4
// source: systemd/systemd.proto

package systemd

import (
	protoreflect "google.golang.org/protobuf/reflect/protoreflect"
	protoimpl "google.golang.org/protobuf/runtime/protoimpl"
	reflect "reflect"
	sync "sync"
	unsafe "unsafe"
)

const (
	// Verify that this generated code is sufficiently up-to-date.
	_ = protoimpl.EnforceVersion(20 - protoimpl.MinVersion)
	// Verify that runtime/protoimpl is sufficiently up-to-date.
	_ = protoimpl.EnforceVersion(protoimpl.MaxVersion - 20)
)

type UnitStatus struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	Name          string                 `protobuf:"bytes,1,opt,name=Name,proto3" json:"Name,omitempty"`
	Description   string                 `protobuf:"bytes,2,opt,name=Description,proto3" json:"Description,omitempty"`
	LoadState     string                 `protobuf:"bytes,3,opt,name=LoadState,proto3" json:"LoadState,omitempty"`
	ActiveState   string                 `protobuf:"bytes,4,opt,name=ActiveState,proto3" json:"ActiveState,omitempty"`
	SubState      string                 `protobuf:"bytes,5,opt,name=SubState,proto3" json:"SubState,omitempty"`
	Path          string                 `protobuf:"bytes,6,opt,name=Path,proto3" json:"Path,omitempty"`
	FreezerState  string                 `protobuf:"bytes,7,opt,name=FreezerState,proto3" json:"FreezerState,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *UnitStatus) Reset() {
	*x = UnitStatus{}
	mi := &file_systemd_systemd_proto_msgTypes[0]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *UnitStatus) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*UnitStatus) ProtoMessage() {}

func (x *UnitStatus) ProtoReflect() protoreflect.Message {
	mi := &file_systemd_systemd_proto_msgTypes[0]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use UnitStatus.ProtoReflect.Descriptor instead.
func (*UnitStatus) Descriptor() ([]byte, []int) {
	return file_systemd_systemd_proto_rawDescGZIP(), []int{0}
}

func (x *UnitStatus) GetName() string {
	if x != nil {
		return x.Name
	}
	return ""
}

func (x *UnitStatus) GetDescription() string {
	if x != nil {
		return x.Description
	}
	return ""
}

func (x *UnitStatus) GetLoadState() string {
	if x != nil {
		return x.LoadState
	}
	return ""
}

func (x *UnitStatus) GetActiveState() string {
	if x != nil {
		return x.ActiveState
	}
	return ""
}

func (x *UnitStatus) GetSubState() string {
	if x != nil {
		return x.SubState
	}
	return ""
}

func (x *UnitStatus) GetPath() string {
	if x != nil {
		return x.Path
	}
	return ""
}

func (x *UnitStatus) GetFreezerState() string {
	if x != nil {
		return x.FreezerState
	}
	return ""
}

type UnitRequest struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	UnitName      string                 `protobuf:"bytes,1,opt,name=UnitName,proto3" json:"UnitName,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *UnitRequest) Reset() {
	*x = UnitRequest{}
	mi := &file_systemd_systemd_proto_msgTypes[1]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *UnitRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*UnitRequest) ProtoMessage() {}

func (x *UnitRequest) ProtoReflect() protoreflect.Message {
	mi := &file_systemd_systemd_proto_msgTypes[1]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use UnitRequest.ProtoReflect.Descriptor instead.
func (*UnitRequest) Descriptor() ([]byte, []int) {
	return file_systemd_systemd_proto_rawDescGZIP(), []int{1}
}

func (x *UnitRequest) GetUnitName() string {
	if x != nil {
		return x.UnitName
	}
	return ""
}

type AppUnitRequest struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	UnitName      string                 `protobuf:"bytes,1,opt,name=UnitName,proto3" json:"UnitName,omitempty"`
	Args          []string               `protobuf:"bytes,2,rep,name=Args,proto3" json:"Args,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *AppUnitRequest) Reset() {
	*x = AppUnitRequest{}
	mi := &file_systemd_systemd_proto_msgTypes[2]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *AppUnitRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*AppUnitRequest) ProtoMessage() {}

func (x *AppUnitRequest) ProtoReflect() protoreflect.Message {
	mi := &file_systemd_systemd_proto_msgTypes[2]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use AppUnitRequest.ProtoReflect.Descriptor instead.
func (*AppUnitRequest) Descriptor() ([]byte, []int) {
	return file_systemd_systemd_proto_rawDescGZIP(), []int{2}
}

func (x *AppUnitRequest) GetUnitName() string {
	if x != nil {
		return x.UnitName
	}
	return ""
}

func (x *AppUnitRequest) GetArgs() []string {
	if x != nil {
		return x.Args
	}
	return nil
}

type UnitResponse struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	UnitStatus    *UnitStatus            `protobuf:"bytes,1,opt,name=UnitStatus,proto3" json:"UnitStatus,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *UnitResponse) Reset() {
	*x = UnitResponse{}
	mi := &file_systemd_systemd_proto_msgTypes[3]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *UnitResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*UnitResponse) ProtoMessage() {}

func (x *UnitResponse) ProtoReflect() protoreflect.Message {
	mi := &file_systemd_systemd_proto_msgTypes[3]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use UnitResponse.ProtoReflect.Descriptor instead.
func (*UnitResponse) Descriptor() ([]byte, []int) {
	return file_systemd_systemd_proto_rawDescGZIP(), []int{3}
}

func (x *UnitResponse) GetUnitStatus() *UnitStatus {
	if x != nil {
		return x.UnitStatus
	}
	return nil
}

type UnitResourceRequest struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	UnitName      string                 `protobuf:"bytes,1,opt,name=UnitName,proto3" json:"UnitName,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *UnitResourceRequest) Reset() {
	*x = UnitResourceRequest{}
	mi := &file_systemd_systemd_proto_msgTypes[4]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *UnitResourceRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*UnitResourceRequest) ProtoMessage() {}

func (x *UnitResourceRequest) ProtoReflect() protoreflect.Message {
	mi := &file_systemd_systemd_proto_msgTypes[4]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use UnitResourceRequest.ProtoReflect.Descriptor instead.
func (*UnitResourceRequest) Descriptor() ([]byte, []int) {
	return file_systemd_systemd_proto_rawDescGZIP(), []int{4}
}

func (x *UnitResourceRequest) GetUnitName() string {
	if x != nil {
		return x.UnitName
	}
	return ""
}

type UnitResourceResponse struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	CpuUsage      float64                `protobuf:"fixed64,1,opt,name=cpu_usage,json=cpuUsage,proto3" json:"cpu_usage,omitempty"`
	MemoryUsage   float32                `protobuf:"fixed32,2,opt,name=memory_usage,json=memoryUsage,proto3" json:"memory_usage,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *UnitResourceResponse) Reset() {
	*x = UnitResourceResponse{}
	mi := &file_systemd_systemd_proto_msgTypes[5]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *UnitResourceResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*UnitResourceResponse) ProtoMessage() {}

func (x *UnitResourceResponse) ProtoReflect() protoreflect.Message {
	mi := &file_systemd_systemd_proto_msgTypes[5]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use UnitResourceResponse.ProtoReflect.Descriptor instead.
func (*UnitResourceResponse) Descriptor() ([]byte, []int) {
	return file_systemd_systemd_proto_rawDescGZIP(), []int{5}
}

func (x *UnitResourceResponse) GetCpuUsage() float64 {
	if x != nil {
		return x.CpuUsage
	}
	return 0
}

func (x *UnitResourceResponse) GetMemoryUsage() float32 {
	if x != nil {
		return x.MemoryUsage
	}
	return 0
}

var File_systemd_systemd_proto protoreflect.FileDescriptor

const file_systemd_systemd_proto_rawDesc = "" +
	"\n" +
	"\x15systemd/systemd.proto\x12\asystemd\"\xd6\x01\n" +
	"\n" +
	"UnitStatus\x12\x12\n" +
	"\x04Name\x18\x01 \x01(\tR\x04Name\x12 \n" +
	"\vDescription\x18\x02 \x01(\tR\vDescription\x12\x1c\n" +
	"\tLoadState\x18\x03 \x01(\tR\tLoadState\x12 \n" +
	"\vActiveState\x18\x04 \x01(\tR\vActiveState\x12\x1a\n" +
	"\bSubState\x18\x05 \x01(\tR\bSubState\x12\x12\n" +
	"\x04Path\x18\x06 \x01(\tR\x04Path\x12\"\n" +
	"\fFreezerState\x18\a \x01(\tR\fFreezerState\")\n" +
	"\vUnitRequest\x12\x1a\n" +
	"\bUnitName\x18\x01 \x01(\tR\bUnitName\"@\n" +
	"\x0eAppUnitRequest\x12\x1a\n" +
	"\bUnitName\x18\x01 \x01(\tR\bUnitName\x12\x12\n" +
	"\x04Args\x18\x02 \x03(\tR\x04Args\"C\n" +
	"\fUnitResponse\x123\n" +
	"\n" +
	"UnitStatus\x18\x01 \x01(\v2\x13.systemd.UnitStatusR\n" +
	"UnitStatus\"1\n" +
	"\x13UnitResourceRequest\x12\x1a\n" +
	"\bUnitName\x18\x01 \x01(\tR\bUnitName\"V\n" +
	"\x14UnitResourceResponse\x12\x1b\n" +
	"\tcpu_usage\x18\x01 \x01(\x01R\bcpuUsage\x12!\n" +
	"\fmemory_usage\x18\x02 \x01(\x02R\vmemoryUsage2\x98\x04\n" +
	"\x12UnitControlService\x12D\n" +
	"\x10StartApplication\x12\x17.systemd.AppUnitRequest\x1a\x15.systemd.UnitResponse\"\x00\x12:\n" +
	"\tStartUnit\x12\x14.systemd.UnitRequest\x1a\x15.systemd.UnitResponse\"\x00\x129\n" +
	"\bStopUnit\x12\x14.systemd.UnitRequest\x1a\x15.systemd.UnitResponse\"\x00\x129\n" +
	"\bKillUnit\x12\x14.systemd.UnitRequest\x1a\x15.systemd.UnitResponse\"\x00\x12;\n" +
	"\n" +
	"FreezeUnit\x12\x14.systemd.UnitRequest\x1a\x15.systemd.UnitResponse\"\x00\x12=\n" +
	"\fUnfreezeUnit\x12\x14.systemd.UnitRequest\x1a\x15.systemd.UnitResponse\"\x00\x12>\n" +
	"\rGetUnitStatus\x12\x14.systemd.UnitRequest\x1a\x15.systemd.UnitResponse\"\x00\x12N\n" +
	"\vMonitorUnit\x12\x1c.systemd.UnitResourceRequest\x1a\x1d.systemd.UnitResourceResponse\"\x000\x01B\x1aZ\x18givc/modules/api/systemdb\x06proto3"

var (
	file_systemd_systemd_proto_rawDescOnce sync.Once
	file_systemd_systemd_proto_rawDescData []byte
)

func file_systemd_systemd_proto_rawDescGZIP() []byte {
	file_systemd_systemd_proto_rawDescOnce.Do(func() {
		file_systemd_systemd_proto_rawDescData = protoimpl.X.CompressGZIP(unsafe.Slice(unsafe.StringData(file_systemd_systemd_proto_rawDesc), len(file_systemd_systemd_proto_rawDesc)))
	})
	return file_systemd_systemd_proto_rawDescData
}

var file_systemd_systemd_proto_msgTypes = make([]protoimpl.MessageInfo, 6)
var file_systemd_systemd_proto_goTypes = []any{
	(*UnitStatus)(nil),           // 0: systemd.UnitStatus
	(*UnitRequest)(nil),          // 1: systemd.UnitRequest
	(*AppUnitRequest)(nil),       // 2: systemd.AppUnitRequest
	(*UnitResponse)(nil),         // 3: systemd.UnitResponse
	(*UnitResourceRequest)(nil),  // 4: systemd.UnitResourceRequest
	(*UnitResourceResponse)(nil), // 5: systemd.UnitResourceResponse
}
var file_systemd_systemd_proto_depIdxs = []int32{
	0, // 0: systemd.UnitResponse.UnitStatus:type_name -> systemd.UnitStatus
	2, // 1: systemd.UnitControlService.StartApplication:input_type -> systemd.AppUnitRequest
	1, // 2: systemd.UnitControlService.StartUnit:input_type -> systemd.UnitRequest
	1, // 3: systemd.UnitControlService.StopUnit:input_type -> systemd.UnitRequest
	1, // 4: systemd.UnitControlService.KillUnit:input_type -> systemd.UnitRequest
	1, // 5: systemd.UnitControlService.FreezeUnit:input_type -> systemd.UnitRequest
	1, // 6: systemd.UnitControlService.UnfreezeUnit:input_type -> systemd.UnitRequest
	1, // 7: systemd.UnitControlService.GetUnitStatus:input_type -> systemd.UnitRequest
	4, // 8: systemd.UnitControlService.MonitorUnit:input_type -> systemd.UnitResourceRequest
	3, // 9: systemd.UnitControlService.StartApplication:output_type -> systemd.UnitResponse
	3, // 10: systemd.UnitControlService.StartUnit:output_type -> systemd.UnitResponse
	3, // 11: systemd.UnitControlService.StopUnit:output_type -> systemd.UnitResponse
	3, // 12: systemd.UnitControlService.KillUnit:output_type -> systemd.UnitResponse
	3, // 13: systemd.UnitControlService.FreezeUnit:output_type -> systemd.UnitResponse
	3, // 14: systemd.UnitControlService.UnfreezeUnit:output_type -> systemd.UnitResponse
	3, // 15: systemd.UnitControlService.GetUnitStatus:output_type -> systemd.UnitResponse
	5, // 16: systemd.UnitControlService.MonitorUnit:output_type -> systemd.UnitResourceResponse
	9, // [9:17] is the sub-list for method output_type
	1, // [1:9] is the sub-list for method input_type
	1, // [1:1] is the sub-list for extension type_name
	1, // [1:1] is the sub-list for extension extendee
	0, // [0:1] is the sub-list for field type_name
}

func init() { file_systemd_systemd_proto_init() }
func file_systemd_systemd_proto_init() {
	if File_systemd_systemd_proto != nil {
		return
	}
	type x struct{}
	out := protoimpl.TypeBuilder{
		File: protoimpl.DescBuilder{
			GoPackagePath: reflect.TypeOf(x{}).PkgPath(),
			RawDescriptor: unsafe.Slice(unsafe.StringData(file_systemd_systemd_proto_rawDesc), len(file_systemd_systemd_proto_rawDesc)),
			NumEnums:      0,
			NumMessages:   6,
			NumExtensions: 0,
			NumServices:   1,
		},
		GoTypes:           file_systemd_systemd_proto_goTypes,
		DependencyIndexes: file_systemd_systemd_proto_depIdxs,
		MessageInfos:      file_systemd_systemd_proto_msgTypes,
	}.Build()
	File_systemd_systemd_proto = out.File
	file_systemd_systemd_proto_goTypes = nil
	file_systemd_systemd_proto_depIdxs = nil
}
