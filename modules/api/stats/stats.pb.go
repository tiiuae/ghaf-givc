// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

// Code generated by protoc-gen-go. DO NOT EDIT.
// versions:
// 	protoc-gen-go v1.36.1
// 	protoc        v5.29.1
// source: stats/stats.proto

package stats

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

type StatsRequest struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *StatsRequest) Reset() {
	*x = StatsRequest{}
	mi := &file_stats_stats_proto_msgTypes[0]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *StatsRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*StatsRequest) ProtoMessage() {}

func (x *StatsRequest) ProtoReflect() protoreflect.Message {
	mi := &file_stats_stats_proto_msgTypes[0]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use StatsRequest.ProtoReflect.Descriptor instead.
func (*StatsRequest) Descriptor() ([]byte, []int) {
	return file_stats_stats_proto_rawDescGZIP(), []int{0}
}

type ProcessStat struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	Name          string                 `protobuf:"bytes,1,opt,name=Name,proto3" json:"Name,omitempty"`
	User          float32                `protobuf:"fixed32,2,opt,name=User,proto3" json:"User,omitempty"`
	Sys           float32                `protobuf:"fixed32,3,opt,name=Sys,proto3" json:"Sys,omitempty"`
	ResSetSize    uint64                 `protobuf:"varint,4,opt,name=ResSetSize,proto3" json:"ResSetSize,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ProcessStat) Reset() {
	*x = ProcessStat{}
	mi := &file_stats_stats_proto_msgTypes[1]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ProcessStat) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ProcessStat) ProtoMessage() {}

func (x *ProcessStat) ProtoReflect() protoreflect.Message {
	mi := &file_stats_stats_proto_msgTypes[1]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ProcessStat.ProtoReflect.Descriptor instead.
func (*ProcessStat) Descriptor() ([]byte, []int) {
	return file_stats_stats_proto_rawDescGZIP(), []int{1}
}

func (x *ProcessStat) GetName() string {
	if x != nil {
		return x.Name
	}
	return ""
}

func (x *ProcessStat) GetUser() float32 {
	if x != nil {
		return x.User
	}
	return 0
}

func (x *ProcessStat) GetSys() float32 {
	if x != nil {
		return x.Sys
	}
	return 0
}

func (x *ProcessStat) GetResSetSize() uint64 {
	if x != nil {
		return x.ResSetSize
	}
	return 0
}

type ProcessStats struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	CpuProcesses  []*ProcessStat         `protobuf:"bytes,1,rep,name=CpuProcesses,proto3" json:"CpuProcesses,omitempty"`
	MemProcesses  []*ProcessStat         `protobuf:"bytes,2,rep,name=MemProcesses,proto3" json:"MemProcesses,omitempty"`
	Total         uint64                 `protobuf:"varint,3,opt,name=Total,proto3" json:"Total,omitempty"`
	Running       uint64                 `protobuf:"varint,4,opt,name=Running,proto3" json:"Running,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ProcessStats) Reset() {
	*x = ProcessStats{}
	mi := &file_stats_stats_proto_msgTypes[2]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ProcessStats) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ProcessStats) ProtoMessage() {}

func (x *ProcessStats) ProtoReflect() protoreflect.Message {
	mi := &file_stats_stats_proto_msgTypes[2]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ProcessStats.ProtoReflect.Descriptor instead.
func (*ProcessStats) Descriptor() ([]byte, []int) {
	return file_stats_stats_proto_rawDescGZIP(), []int{2}
}

func (x *ProcessStats) GetCpuProcesses() []*ProcessStat {
	if x != nil {
		return x.CpuProcesses
	}
	return nil
}

func (x *ProcessStats) GetMemProcesses() []*ProcessStat {
	if x != nil {
		return x.MemProcesses
	}
	return nil
}

func (x *ProcessStats) GetTotal() uint64 {
	if x != nil {
		return x.Total
	}
	return 0
}

func (x *ProcessStats) GetRunning() uint64 {
	if x != nil {
		return x.Running
	}
	return 0
}

type LoadStats struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	Load1Min      float32                `protobuf:"fixed32,1,opt,name=Load1Min,proto3" json:"Load1Min,omitempty"`
	Load5Min      float32                `protobuf:"fixed32,2,opt,name=Load5Min,proto3" json:"Load5Min,omitempty"`
	Load15Min     float32                `protobuf:"fixed32,3,opt,name=Load15Min,proto3" json:"Load15Min,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *LoadStats) Reset() {
	*x = LoadStats{}
	mi := &file_stats_stats_proto_msgTypes[3]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *LoadStats) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*LoadStats) ProtoMessage() {}

func (x *LoadStats) ProtoReflect() protoreflect.Message {
	mi := &file_stats_stats_proto_msgTypes[3]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use LoadStats.ProtoReflect.Descriptor instead.
func (*LoadStats) Descriptor() ([]byte, []int) {
	return file_stats_stats_proto_rawDescGZIP(), []int{3}
}

func (x *LoadStats) GetLoad1Min() float32 {
	if x != nil {
		return x.Load1Min
	}
	return 0
}

func (x *LoadStats) GetLoad5Min() float32 {
	if x != nil {
		return x.Load5Min
	}
	return 0
}

func (x *LoadStats) GetLoad15Min() float32 {
	if x != nil {
		return x.Load15Min
	}
	return 0
}

type MemoryStats struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	Total         uint64                 `protobuf:"varint,1,opt,name=Total,proto3" json:"Total,omitempty"`
	Free          uint64                 `protobuf:"varint,2,opt,name=Free,proto3" json:"Free,omitempty"`
	Available     uint64                 `protobuf:"varint,3,opt,name=Available,proto3" json:"Available,omitempty"`
	Cached        uint64                 `protobuf:"varint,4,opt,name=Cached,proto3" json:"Cached,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *MemoryStats) Reset() {
	*x = MemoryStats{}
	mi := &file_stats_stats_proto_msgTypes[4]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *MemoryStats) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*MemoryStats) ProtoMessage() {}

func (x *MemoryStats) ProtoReflect() protoreflect.Message {
	mi := &file_stats_stats_proto_msgTypes[4]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use MemoryStats.ProtoReflect.Descriptor instead.
func (*MemoryStats) Descriptor() ([]byte, []int) {
	return file_stats_stats_proto_rawDescGZIP(), []int{4}
}

func (x *MemoryStats) GetTotal() uint64 {
	if x != nil {
		return x.Total
	}
	return 0
}

func (x *MemoryStats) GetFree() uint64 {
	if x != nil {
		return x.Free
	}
	return 0
}

func (x *MemoryStats) GetAvailable() uint64 {
	if x != nil {
		return x.Available
	}
	return 0
}

func (x *MemoryStats) GetCached() uint64 {
	if x != nil {
		return x.Cached
	}
	return 0
}

type StatsResponse struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	Memory        *MemoryStats           `protobuf:"bytes,1,opt,name=Memory,proto3" json:"Memory,omitempty"`
	Load          *LoadStats             `protobuf:"bytes,2,opt,name=Load,proto3" json:"Load,omitempty"`
	Process       *ProcessStats          `protobuf:"bytes,3,opt,name=Process,proto3" json:"Process,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *StatsResponse) Reset() {
	*x = StatsResponse{}
	mi := &file_stats_stats_proto_msgTypes[5]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *StatsResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*StatsResponse) ProtoMessage() {}

func (x *StatsResponse) ProtoReflect() protoreflect.Message {
	mi := &file_stats_stats_proto_msgTypes[5]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use StatsResponse.ProtoReflect.Descriptor instead.
func (*StatsResponse) Descriptor() ([]byte, []int) {
	return file_stats_stats_proto_rawDescGZIP(), []int{5}
}

func (x *StatsResponse) GetMemory() *MemoryStats {
	if x != nil {
		return x.Memory
	}
	return nil
}

func (x *StatsResponse) GetLoad() *LoadStats {
	if x != nil {
		return x.Load
	}
	return nil
}

func (x *StatsResponse) GetProcess() *ProcessStats {
	if x != nil {
		return x.Process
	}
	return nil
}

var File_stats_stats_proto protoreflect.FileDescriptor

var file_stats_stats_proto_rawDesc = []byte{
	0x0a, 0x11, 0x73, 0x74, 0x61, 0x74, 0x73, 0x2f, 0x73, 0x74, 0x61, 0x74, 0x73, 0x2e, 0x70, 0x72,
	0x6f, 0x74, 0x6f, 0x12, 0x05, 0x73, 0x74, 0x61, 0x74, 0x73, 0x22, 0x0e, 0x0a, 0x0c, 0x53, 0x74,
	0x61, 0x74, 0x73, 0x52, 0x65, 0x71, 0x75, 0x65, 0x73, 0x74, 0x22, 0x67, 0x0a, 0x0b, 0x50, 0x72,
	0x6f, 0x63, 0x65, 0x73, 0x73, 0x53, 0x74, 0x61, 0x74, 0x12, 0x12, 0x0a, 0x04, 0x4e, 0x61, 0x6d,
	0x65, 0x18, 0x01, 0x20, 0x01, 0x28, 0x09, 0x52, 0x04, 0x4e, 0x61, 0x6d, 0x65, 0x12, 0x12, 0x0a,
	0x04, 0x55, 0x73, 0x65, 0x72, 0x18, 0x02, 0x20, 0x01, 0x28, 0x02, 0x52, 0x04, 0x55, 0x73, 0x65,
	0x72, 0x12, 0x10, 0x0a, 0x03, 0x53, 0x79, 0x73, 0x18, 0x03, 0x20, 0x01, 0x28, 0x02, 0x52, 0x03,
	0x53, 0x79, 0x73, 0x12, 0x1e, 0x0a, 0x0a, 0x52, 0x65, 0x73, 0x53, 0x65, 0x74, 0x53, 0x69, 0x7a,
	0x65, 0x18, 0x04, 0x20, 0x01, 0x28, 0x04, 0x52, 0x0a, 0x52, 0x65, 0x73, 0x53, 0x65, 0x74, 0x53,
	0x69, 0x7a, 0x65, 0x22, 0xae, 0x01, 0x0a, 0x0c, 0x50, 0x72, 0x6f, 0x63, 0x65, 0x73, 0x73, 0x53,
	0x74, 0x61, 0x74, 0x73, 0x12, 0x36, 0x0a, 0x0c, 0x43, 0x70, 0x75, 0x50, 0x72, 0x6f, 0x63, 0x65,
	0x73, 0x73, 0x65, 0x73, 0x18, 0x01, 0x20, 0x03, 0x28, 0x0b, 0x32, 0x12, 0x2e, 0x73, 0x74, 0x61,
	0x74, 0x73, 0x2e, 0x50, 0x72, 0x6f, 0x63, 0x65, 0x73, 0x73, 0x53, 0x74, 0x61, 0x74, 0x52, 0x0c,
	0x43, 0x70, 0x75, 0x50, 0x72, 0x6f, 0x63, 0x65, 0x73, 0x73, 0x65, 0x73, 0x12, 0x36, 0x0a, 0x0c,
	0x4d, 0x65, 0x6d, 0x50, 0x72, 0x6f, 0x63, 0x65, 0x73, 0x73, 0x65, 0x73, 0x18, 0x02, 0x20, 0x03,
	0x28, 0x0b, 0x32, 0x12, 0x2e, 0x73, 0x74, 0x61, 0x74, 0x73, 0x2e, 0x50, 0x72, 0x6f, 0x63, 0x65,
	0x73, 0x73, 0x53, 0x74, 0x61, 0x74, 0x52, 0x0c, 0x4d, 0x65, 0x6d, 0x50, 0x72, 0x6f, 0x63, 0x65,
	0x73, 0x73, 0x65, 0x73, 0x12, 0x14, 0x0a, 0x05, 0x54, 0x6f, 0x74, 0x61, 0x6c, 0x18, 0x03, 0x20,
	0x01, 0x28, 0x04, 0x52, 0x05, 0x54, 0x6f, 0x74, 0x61, 0x6c, 0x12, 0x18, 0x0a, 0x07, 0x52, 0x75,
	0x6e, 0x6e, 0x69, 0x6e, 0x67, 0x18, 0x04, 0x20, 0x01, 0x28, 0x04, 0x52, 0x07, 0x52, 0x75, 0x6e,
	0x6e, 0x69, 0x6e, 0x67, 0x22, 0x61, 0x0a, 0x09, 0x4c, 0x6f, 0x61, 0x64, 0x53, 0x74, 0x61, 0x74,
	0x73, 0x12, 0x1a, 0x0a, 0x08, 0x4c, 0x6f, 0x61, 0x64, 0x31, 0x4d, 0x69, 0x6e, 0x18, 0x01, 0x20,
	0x01, 0x28, 0x02, 0x52, 0x08, 0x4c, 0x6f, 0x61, 0x64, 0x31, 0x4d, 0x69, 0x6e, 0x12, 0x1a, 0x0a,
	0x08, 0x4c, 0x6f, 0x61, 0x64, 0x35, 0x4d, 0x69, 0x6e, 0x18, 0x02, 0x20, 0x01, 0x28, 0x02, 0x52,
	0x08, 0x4c, 0x6f, 0x61, 0x64, 0x35, 0x4d, 0x69, 0x6e, 0x12, 0x1c, 0x0a, 0x09, 0x4c, 0x6f, 0x61,
	0x64, 0x31, 0x35, 0x4d, 0x69, 0x6e, 0x18, 0x03, 0x20, 0x01, 0x28, 0x02, 0x52, 0x09, 0x4c, 0x6f,
	0x61, 0x64, 0x31, 0x35, 0x4d, 0x69, 0x6e, 0x22, 0x6d, 0x0a, 0x0b, 0x4d, 0x65, 0x6d, 0x6f, 0x72,
	0x79, 0x53, 0x74, 0x61, 0x74, 0x73, 0x12, 0x14, 0x0a, 0x05, 0x54, 0x6f, 0x74, 0x61, 0x6c, 0x18,
	0x01, 0x20, 0x01, 0x28, 0x04, 0x52, 0x05, 0x54, 0x6f, 0x74, 0x61, 0x6c, 0x12, 0x12, 0x0a, 0x04,
	0x46, 0x72, 0x65, 0x65, 0x18, 0x02, 0x20, 0x01, 0x28, 0x04, 0x52, 0x04, 0x46, 0x72, 0x65, 0x65,
	0x12, 0x1c, 0x0a, 0x09, 0x41, 0x76, 0x61, 0x69, 0x6c, 0x61, 0x62, 0x6c, 0x65, 0x18, 0x03, 0x20,
	0x01, 0x28, 0x04, 0x52, 0x09, 0x41, 0x76, 0x61, 0x69, 0x6c, 0x61, 0x62, 0x6c, 0x65, 0x12, 0x16,
	0x0a, 0x06, 0x43, 0x61, 0x63, 0x68, 0x65, 0x64, 0x18, 0x04, 0x20, 0x01, 0x28, 0x04, 0x52, 0x06,
	0x43, 0x61, 0x63, 0x68, 0x65, 0x64, 0x22, 0x90, 0x01, 0x0a, 0x0d, 0x53, 0x74, 0x61, 0x74, 0x73,
	0x52, 0x65, 0x73, 0x70, 0x6f, 0x6e, 0x73, 0x65, 0x12, 0x2a, 0x0a, 0x06, 0x4d, 0x65, 0x6d, 0x6f,
	0x72, 0x79, 0x18, 0x01, 0x20, 0x01, 0x28, 0x0b, 0x32, 0x12, 0x2e, 0x73, 0x74, 0x61, 0x74, 0x73,
	0x2e, 0x4d, 0x65, 0x6d, 0x6f, 0x72, 0x79, 0x53, 0x74, 0x61, 0x74, 0x73, 0x52, 0x06, 0x4d, 0x65,
	0x6d, 0x6f, 0x72, 0x79, 0x12, 0x24, 0x0a, 0x04, 0x4c, 0x6f, 0x61, 0x64, 0x18, 0x02, 0x20, 0x01,
	0x28, 0x0b, 0x32, 0x10, 0x2e, 0x73, 0x74, 0x61, 0x74, 0x73, 0x2e, 0x4c, 0x6f, 0x61, 0x64, 0x53,
	0x74, 0x61, 0x74, 0x73, 0x52, 0x04, 0x4c, 0x6f, 0x61, 0x64, 0x12, 0x2d, 0x0a, 0x07, 0x50, 0x72,
	0x6f, 0x63, 0x65, 0x73, 0x73, 0x18, 0x03, 0x20, 0x01, 0x28, 0x0b, 0x32, 0x13, 0x2e, 0x73, 0x74,
	0x61, 0x74, 0x73, 0x2e, 0x50, 0x72, 0x6f, 0x63, 0x65, 0x73, 0x73, 0x53, 0x74, 0x61, 0x74, 0x73,
	0x52, 0x07, 0x50, 0x72, 0x6f, 0x63, 0x65, 0x73, 0x73, 0x32, 0x47, 0x0a, 0x0c, 0x53, 0x74, 0x61,
	0x74, 0x73, 0x53, 0x65, 0x72, 0x76, 0x69, 0x63, 0x65, 0x12, 0x37, 0x0a, 0x08, 0x47, 0x65, 0x74,
	0x53, 0x74, 0x61, 0x74, 0x73, 0x12, 0x13, 0x2e, 0x73, 0x74, 0x61, 0x74, 0x73, 0x2e, 0x53, 0x74,
	0x61, 0x74, 0x73, 0x52, 0x65, 0x71, 0x75, 0x65, 0x73, 0x74, 0x1a, 0x14, 0x2e, 0x73, 0x74, 0x61,
	0x74, 0x73, 0x2e, 0x53, 0x74, 0x61, 0x74, 0x73, 0x52, 0x65, 0x73, 0x70, 0x6f, 0x6e, 0x73, 0x65,
	0x22, 0x00, 0x42, 0x18, 0x5a, 0x16, 0x67, 0x69, 0x76, 0x63, 0x2f, 0x6d, 0x6f, 0x64, 0x75, 0x6c,
	0x65, 0x73, 0x2f, 0x61, 0x70, 0x69, 0x2f, 0x73, 0x74, 0x61, 0x74, 0x73, 0x62, 0x06, 0x70, 0x72,
	0x6f, 0x74, 0x6f, 0x33,
}

var (
	file_stats_stats_proto_rawDescOnce sync.Once
	file_stats_stats_proto_rawDescData = file_stats_stats_proto_rawDesc
)

func file_stats_stats_proto_rawDescGZIP() []byte {
	file_stats_stats_proto_rawDescOnce.Do(func() {
		file_stats_stats_proto_rawDescData = protoimpl.X.CompressGZIP(file_stats_stats_proto_rawDescData)
	})
	return file_stats_stats_proto_rawDescData
}

var file_stats_stats_proto_msgTypes = make([]protoimpl.MessageInfo, 6)
var file_stats_stats_proto_goTypes = []any{
	(*StatsRequest)(nil),  // 0: stats.StatsRequest
	(*ProcessStat)(nil),   // 1: stats.ProcessStat
	(*ProcessStats)(nil),  // 2: stats.ProcessStats
	(*LoadStats)(nil),     // 3: stats.LoadStats
	(*MemoryStats)(nil),   // 4: stats.MemoryStats
	(*StatsResponse)(nil), // 5: stats.StatsResponse
}
var file_stats_stats_proto_depIdxs = []int32{
	1, // 0: stats.ProcessStats.CpuProcesses:type_name -> stats.ProcessStat
	1, // 1: stats.ProcessStats.MemProcesses:type_name -> stats.ProcessStat
	4, // 2: stats.StatsResponse.Memory:type_name -> stats.MemoryStats
	3, // 3: stats.StatsResponse.Load:type_name -> stats.LoadStats
	2, // 4: stats.StatsResponse.Process:type_name -> stats.ProcessStats
	0, // 5: stats.StatsService.GetStats:input_type -> stats.StatsRequest
	5, // 6: stats.StatsService.GetStats:output_type -> stats.StatsResponse
	6, // [6:7] is the sub-list for method output_type
	5, // [5:6] is the sub-list for method input_type
	5, // [5:5] is the sub-list for extension type_name
	5, // [5:5] is the sub-list for extension extendee
	0, // [0:5] is the sub-list for field type_name
}

func init() { file_stats_stats_proto_init() }
func file_stats_stats_proto_init() {
	if File_stats_stats_proto != nil {
		return
	}
	type x struct{}
	out := protoimpl.TypeBuilder{
		File: protoimpl.DescBuilder{
			GoPackagePath: reflect.TypeOf(x{}).PkgPath(),
			RawDescriptor: file_stats_stats_proto_rawDesc,
			NumEnums:      0,
			NumMessages:   6,
			NumExtensions: 0,
			NumServices:   1,
		},
		GoTypes:           file_stats_stats_proto_goTypes,
		DependencyIndexes: file_stats_stats_proto_depIdxs,
		MessageInfos:      file_stats_stats_proto_msgTypes,
	}.Build()
	File_stats_stats_proto = out.File
	file_stats_stats_proto_rawDesc = nil
	file_stats_stats_proto_goTypes = nil
	file_stats_stats_proto_depIdxs = nil
}
