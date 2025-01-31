// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package applications

import (
	"givc/modules/pkgs/types"
	"os"
	"reflect"
	"testing"
)

func Test_validateServiceName(t *testing.T) {
	type args struct {
		serviceName string
	}
	tests := []struct {
		name    string
		args    args
		wantErr bool
	}{
		{"valid service name", args{serviceName: "-@-.service"}, false},
		{"valid service name", args{serviceName: "1@1.service"}, false},
		{"valid service name", args{serviceName: "-_1@1_-.service"}, false},
		{"valid service name", args{serviceName: "valid@my-service.service"}, false},
		{"valid service name", args{serviceName: "valid@my.service"}, false},

		{"invalid service name", args{serviceName: "a@valid@my-service.service"}, true},
		{"invalid service name", args{serviceName: "a@a@my-service.service"}, true},
		{"invalid service name", args{serviceName: "a@valid.service@my-service.service"}, true},
		{"invalid service name", args{serviceName: "@1.service"}, true},
		{"invalid service name", args{serviceName: "my-service.service"}, true},
		{"invalid service name", args{serviceName: "my-service@.service"}, true},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if err := validateServiceName(tt.args.serviceName); (err != nil) != tt.wantErr {
				t.Errorf("validateServiceName() error = %v, wantErr %v", err, tt.wantErr)
			}
		})
	}
}

func Test_validateUrl(t *testing.T) {
	type args struct {
		urlString string
	}
	tests := []struct {
		name    string
		args    args
		wantErr bool
	}{
		// Valid URLs
		{"valid url", args{urlString: "http://example.com"}, false},
		{"valid url", args{urlString: "https://example.com"}, false},
		{"valid url", args{urlString: "https://example.com/something."}, false},
		{"valid url", args{urlString: "https://example.com?q=2#first"}, false},
		{"valid url", args{urlString: "https://localhost:8080/"}, false},

		// Invalid URLs
		{"invalid protocol", args{urlString: "file:///etc/passwd"}, true},
		{"inject shell cmd", args{urlString: "https://example.com$(touch IWASHERE)"}, true},
		{"inject shell cmd", args{urlString: "https://example.com`touch IWASHERE`"}, true},
		{"inject shell cmd", args{urlString: "https://example.com/ $(touch IWASHERE)"}, true},
		{"inject shell cmd", args{urlString: "$(touch IWASHERE)https://google.com"}, true},
		{"inject shell cmd", args{urlString: "https://example.com\\%20$(touch IWASHERE)"}, true},
		{"inject shell cmd", args{urlString: "https://example.com/a;\\$(touch IWASHERE)"}, true},
		{"inject shell cmd", args{urlString: "https://example.com;touch IWASHERE"}, true},
		{"inject shell cmd", args{urlString: "https://example.com/'$(touch IWASHERE)"}, true},
		{"user info in url", args{urlString: "https://bob:pass123@www.example.com/"}, true},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if err := validateUrl(tt.args.urlString); (err != nil) != tt.wantErr {
				t.Errorf("validateUrl() error = %v, wantErr %v", err, tt.wantErr)
			}
		})
	}
}

func Test_validateFilePath(t *testing.T) {

	f1, err := os.CreateTemp("", "file")
	if err != nil {
		t.Errorf("Error creating test file: %v", err)
	}
	defer os.Remove(f1.Name())
	f2, err := os.CreateTemp("", ".file")
	if err != nil {
		t.Errorf("Error creating test file: %v", err)
	}
	defer os.Remove(f2.Name())
	f3, err := os.CreateTemp("", "another file")
	if err != nil {
		t.Errorf("Error creating test file: %v", err)
	}
	defer os.Remove(f3.Name())
	f4, err := os.CreateTemp("", "another{2}[1]file(4).txt")
	if err != nil {
		t.Errorf("Error creating test file: %v", err)
	}
	defer os.Remove(f4.Name())

	type args struct {
		path        string
		directories []string
	}
	tests := []struct {
		name    string
		args    args
		wantErr bool
	}{
		// Valid file paths
		{"valid file path", args{path: f1.Name(), directories: []string{os.TempDir()}}, false},
		{"valid file path", args{path: f2.Name(), directories: []string{os.TempDir()}}, false},
		{"valid file path", args{path: f3.Name(), directories: []string{os.TempDir()}}, false},
		{"valid file path", args{path: f4.Name(), directories: []string{os.TempDir()}}, false},

		// Invalid directory
		{"valid file path", args{path: f1.Name(), directories: []string{"tmp"}}, true},
		{"valid file path", args{path: f1.Name(), directories: nil}, true},
		{"valid file path", args{path: f1.Name(), directories: []string{}}, true},
		{"valid file path", args{path: f1.Name(), directories: []string{"/etc"}}, true},

		// Invalid file paths
		{"invalid file path", args{path: "//something.txt", directories: []string{"/etc"}}, true},
		{"invalid file path", args{path: "../../something.txt", directories: []string{"/etc"}}, true},
		{"invalid file path", args{path: "../something.txt", directories: []string{"/etc"}}, true},
		{"invalid file path", args{path: "//../../something.txt ", directories: []string{"/etc"}}, true},
		{"invalid file path", args{path: "/a/b/c/../something.txt ", directories: []string{"/etc"}}, true},
		{"invalid file path", args{path: "/etc/password $(touch /etc/something)", directories: []string{"/etc"}}, true},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if err := validateFilePath(tt.args.path, tt.args.directories); (err != nil) != tt.wantErr {
				t.Errorf("validateFilePath() error = %v, wantErr %v", err, tt.wantErr)
			}
		})

	}
}

func TestParseApplicationManifests(t *testing.T) {
	type args struct {
		jsonApplicationString string
	}
	tests := []struct {
		name    string
		args    args
		want    []types.ApplicationManifest
		wantErr bool
	}{
		// Valid cases
		{
			"single application",
			args{`[{
				"Name":"test-app",
				"Command":"chromium",
				"Args":["url"]
			}]`},
			[]types.ApplicationManifest{
				{
					Name:    "test-app",
					Command: "chromium",
					Args:    []string{types.APP_ARG_URL},
				},
			},
			false,
		},
		{
			"single application without args",
			args{`[{"Name":"test-app","Command":"chromium"}]`},
			[]types.ApplicationManifest{
				{
					Name:    "test-app",
					Command: "chromium",
					Args:    nil,
				},
			},
			false,
		},
		{
			"single application with two args",
			args{`[{"Name":"test-app","Command":"chromium","Args":["flag","url"]}]`},
			[]types.ApplicationManifest{
				{
					Name:    "test-app",
					Command: "chromium",
					Args:    []string{types.APP_ARG_FLAG, types.APP_ARG_URL},
				},
			},
			false,
		},
		{
			"two applications with args",
			args{`[
				{"Name":"test-app","Command":"chromium","Args":["flag","url"]},
				{"Name":"test-app2","Command":"firefox","Args":["url"]}
			]`},
			[]types.ApplicationManifest{
				{
					Name:    "test-app",
					Command: "chromium",
					Args:    []string{types.APP_ARG_FLAG, types.APP_ARG_URL},
				},
				{
					Name:    "test-app2",
					Command: "firefox",
					Args:    []string{types.APP_ARG_URL},
				},
			},
			false,
		},
		{
			"two applications without args",
			args{`[
				{"Name":"test-app","Command":"chromium"},
				{"Name":"test-app2","Command":"firefox"}
			]`},
			[]types.ApplicationManifest{
				{
					Name:    "test-app",
					Command: "chromium",
					Args:    nil,
				},
				{
					Name:    "test-app2",
					Command: "firefox",
					Args:    nil,
				},
			},
			false,
		},
		{
			"two applications with file args",
			args{`[
				{"Name":"test-app","Command":"chromium","Args":["flag","url","file"],"Directories":["/tmp"]},
				{"Name":"test-app2","Command":"firefox","Args":["file"],"Directories":["/tmp"]}
			]`},
			[]types.ApplicationManifest{
				{
					Name:        "test-app",
					Command:     "chromium",
					Args:        []string{types.APP_ARG_FLAG, types.APP_ARG_URL, types.APP_ARG_FILE},
					Directories: []string{"/tmp"},
				},
				{
					Name:        "test-app2",
					Command:     "firefox",
					Args:        []string{types.APP_ARG_FILE},
					Directories: []string{"/tmp"},
				},
			},
			false,
		},

		// Error cases
		{
			"single application with wrong arg",
			args{`[{"Name":"test-app","Command":"chromium", "Args":["argument"]}]`},
			nil,
			true,
		},
		{
			"single application two args one wrong",
			args{`[{"Name":"test-app","Command":"chromium", "Args":["url", "argument"]}]`},
			nil,
			true,
		},
		{
			"two applications, with wrong args",
			args{`[
				{"Name":"test-app","Command":"chromium", "Args":["url", "argument"]"},
				{"Name":"test-app2","Command":"firefox", "Args":["url", "argument"]"},
			]`},
			nil,
			true,
		},
		{
			"two applications, same name",
			args{`[
				{"Name":"test-app","Command":"chromium"},
				{"Name":"test-app","Command":"firefox"}
			]`},
			nil,
			true,
		},
		{
			"one applications, missing directories but file flag",
			args{`[
				{"Name":"test-app","Command":"chromium", Args:["file"]},
			]`},
			nil,
			true,
		},
		{
			"two applications, one without file args",
			args{`[
				{"Name":"test-app","Command":"chromium","Args":["flag","url"],"Directories":["/tmp"]},
				{"Name":"test-app2","Command":"firefox","Args":["file"],"Directories":["/tmp"]},
			]`},
			nil,
			true,
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := ParseApplicationManifests(tt.args.jsonApplicationString)
			if (err != nil) != tt.wantErr {
				t.Errorf("ParseApplicationManifests() error = %v, wantErr %v", err, tt.wantErr)
				return
			}
			if !reflect.DeepEqual(got, tt.want) {
				t.Errorf("ParseApplicationManifests() = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestValidateAppUnitRequest(t *testing.T) {
	type args struct {
		serviceName  string
		serviceArgs  []string
		applications []types.ApplicationManifest
	}
	tests := []struct {
		name    string
		args    args
		wantErr bool
	}{
		// Valid cases
		{
			"Simple app request",
			args{
				serviceName: "test-app@1.service",
				serviceArgs: nil,
				applications: []types.ApplicationManifest{
					{
						Name:    "test-app",
						Command: "chromium",
						Args:    nil,
					},
				},
			},
			false,
		},
		{
			"App request with args",
			args{
				serviceName: "test-app@1.service",
				serviceArgs: []string{"https://example.com", "--incognito"},
				applications: []types.ApplicationManifest{
					{
						Name:    "test-app",
						Command: "chromium",
						Args:    []string{types.APP_ARG_URL, types.APP_ARG_FLAG},
					},
				},
			},
			false,
		},
		{
			"App request single arg with more allowed",
			args{
				serviceName: "test-app@1.service",
				serviceArgs: []string{"-incognito", "--incognito"},
				applications: []types.ApplicationManifest{
					{
						Name:    "test-app",
						Command: "chromium",
						Args:    []string{types.APP_ARG_URL, types.APP_ARG_FLAG},
					},
				},
			},
			false,
		},

		// Error cases

		{
			"App request, wrong service name",
			args{
				serviceName: "test-app3@1.service",
				serviceArgs: nil,
				applications: []types.ApplicationManifest{
					{
						Name:    "test-app",
						Command: "chromium",
						Args:    nil,
					},
					{
						Name:    "test-app2",
						Command: "firefox",
						Args:    nil,
					},
				},
			},
			true,
		},
		{
			"App request with wrong arg content",
			args{
				serviceName: "test-app@1.service",
				serviceArgs: []string{"--incognito;echo something"},
				applications: []types.ApplicationManifest{
					{
						Name:    "test-app",
						Command: "chromium",
						Args:    []string{types.APP_ARG_URL, types.APP_ARG_FLAG},
					},
				},
			},
			true,
		},
		{
			"App request with wrong arg content",
			args{
				serviceName: "test-app@1.service",
				serviceArgs: []string{"--incognito;echo something"},
				applications: []types.ApplicationManifest{
					{
						Name:    "test-app",
						Command: "chromium",
						Args:    []string{types.APP_ARG_URL, types.APP_ARG_FLAG},
					},
				},
			},
			true,
		},
		{
			"App request to wrong service",
			args{
				serviceName: "test-app2@1.service",
				serviceArgs: []string{"https://example.com", "--incognito"},
				applications: []types.ApplicationManifest{
					{
						Name:    "test-app",
						Command: "chromium",
						Args:    []string{types.APP_ARG_URL, types.APP_ARG_FLAG},
					},
					{
						Name:    "test-app2",
						Command: "firefox",
						Args:    nil,
					},
				},
			},
			true,
		},

		{
			"Wrong service name test",
			args{
				serviceName:  "test-app2@1.service",
				serviceArgs:  nil,
				applications: nil,
			},
			true,
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if err := ValidateAppUnitRequest(tt.args.serviceName, tt.args.serviceArgs, tt.args.applications); (err != nil) != tt.wantErr {
				t.Errorf("ValidateAppUnitRequest() error = %v, wantErr %v", err, tt.wantErr)
			}
		})
	}
}
