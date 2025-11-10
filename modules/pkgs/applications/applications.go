// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

// The application package provides functionality to parse and validate application manifests and
// runtime arguments.
package applications

import (
	"fmt"
	"givc/modules/pkgs/types"
	"givc/modules/pkgs/utility"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"slices"
	"strings"

	log "github.com/sirupsen/logrus"

	validation "github.com/go-ozzo/ozzo-validation/v4"
	"github.com/go-ozzo/ozzo-validation/v4/is"
)

var (
	UrlExtraSchemes = []string{
		"element",
		"io.element.desktop",
	}
)

// validateServiceName checks if the service name is valid according to the specified format.
func validateServiceName(serviceName string) error {
	return validation.Validate(
		serviceName,
		validation.Required,
		is.PrintableASCII,
		validation.Match(regexp.MustCompile(`^[a-zA-Z0-9_-]+@[a-zA-Z0-9_-]+\.service$`)),
	)
}

// validateFilePath checks if the file path is valid and exists in the specified directories.
func validateFilePath(filePathString string, directories []string) error {
	err := validation.Validate(
		filePathString,
		validation.Required,
		validation.Match(regexp.MustCompile(`^/[-a-zA-Z0-9_/\.\ \(\)\[\]\{\}]+$`)),
	)
	if err != nil {
		log.Warnf("Invalid file path in args: %s Error: %v", filePathString, err)
		return fmt.Errorf("failure parsing file path")
	}
	re := regexp.MustCompile(`\.\./`)
	if re.MatchString(filePathString) {
		log.Warnf("Invalid file path in args: %s", filePathString)
		return fmt.Errorf("failure parsing file path")
	}
	// Extract path and check argument
	if filepath.Clean(filePathString) != filePathString {
		log.Warnf("Error cleaning file path: %s", filePathString)
		return fmt.Errorf("failure parsing file path")
	}
	// Verify that file exists
	for _, dir := range directories {
		if strings.HasPrefix(filePathString, dir) {
			_, err := os.Stat(filePathString)
			return err
		}
	}
	return fmt.Errorf("failure parsing file path")
}

// validateUrl checks if the URL is valid and has a valid scheme.
func validateUrl(urlString string) error {

	// Replace additional URL schemes with 'http' to
	// allow passing URL validation via govalidator
	validationUrl := urlString
	for _, prefix := range UrlExtraSchemes {
		if after, found := strings.CutPrefix(validationUrl, prefix+":"); found {
			validationUrl = "http://" + strings.TrimLeft(after, "/")
			break
		}
	}

	// Basic URL validation
	err := validation.Validate(
		validationUrl,
		validation.Required,
		is.URL,
	)
	if err != nil {
		log.Warnf("Invalid URL in args: %s Error: %v", urlString, err)
		return fmt.Errorf("failure in parsing URL")
	}

	// Additional URL checks
	reqUrl, err := url.Parse(urlString)
	if err != nil {
		log.Warnf("Invalid URL in args: %s", urlString)
		return fmt.Errorf("failure in parsing URL")
	}
	if validationUrl == urlString && (reqUrl.Scheme != "http" && reqUrl.Scheme != "https") {
		log.Warnf("Non-allowed scheme in URL: %s", reqUrl.Scheme)
		return fmt.Errorf("failure in parsing URL")
	}
	if reqUrl.User != nil {
		log.Warnf("User info in URL: %s", reqUrl.User)
		return fmt.Errorf("failure in parsing URL")
	}
	return nil
}

// validateApplicationArgs checks if the application arguments are valid according to the specified types,
// and subsequently triggers individual validation functions for each type.
func validateApplicationArgs(args []string, allowedArgs []string, directories []string) error {

	checkAllowed := func(err error, argType string, allowedArgs []string) bool {
		if err == nil {
			return utility.CheckStringInArray(argType, allowedArgs)
		}
		return false
	}

	// Check if arg is allowed
	var err error
	for _, arg := range args {
		err = validation.Validate(
			arg,
			validation.Required,
			is.PrintableASCII,
			validation.Match(regexp.MustCompile(`^-[-]?[a-zA-Z0-9_-]+$`)),
		)
		valid := checkAllowed(err, types.APP_ARG_FLAG, allowedArgs)
		if valid {
			continue
		}

		err = validateUrl(arg)
		valid = checkAllowed(err, types.APP_ARG_URL, allowedArgs)
		if valid {
			continue
		}

		err = validateFilePath(arg, directories)
		valid = checkAllowed(err, types.APP_ARG_FILE, allowedArgs)
		if valid {
			continue
		}
		return fmt.Errorf("invalid application argument: %s", arg)
	}
	return nil
}

// ValidateApplicationManifests validates the application manifests.
func ValidateApplicationManifests(applications []types.ApplicationManifest) error {

	// Verify application manifest formats
	appNames := []string{}
	for _, app := range applications {
		// Check app name not empty
		if app.Name == "" {
			return fmt.Errorf("application name is empty")
		}
		if slices.Contains(appNames, app.Name) {
			return fmt.Errorf("duplicate application name")
		}
		appNames = append(appNames, app.Name)

		// Check app command not empty
		if app.Command == "" {
			return fmt.Errorf("application command is empty")
		}

		// Check directory argument implies file flag
		if len(app.Directories) > 0 {
			if app.Args == nil || !slices.Contains(app.Args, types.APP_ARG_FILE) {
				return fmt.Errorf("directories specified but file argument type missing")
			}
		}

		// Check app args types
		if app.Args != nil {
			for _, argType := range app.Args {
				switch argType {
				case types.APP_ARG_FLAG:
				case types.APP_ARG_URL:
				case types.APP_ARG_FILE:
					if len(app.Directories) == 0 {
						return fmt.Errorf("file argument given but no directories specified")
					}
				default:
					return fmt.Errorf("application argument type not supported")
				}
			}
		}
	}
	return nil
}

// ValidateAppUnitRequest validates the application unit request by checking the service name format,
// and verifying the application arguments against the manifest.
func ValidateAppUnitRequest(serviceName string, appArgs []string, applications []types.ApplicationManifest) error {

	// Verify application request
	name := strings.Split(serviceName, "@")[0]
	validEntryFound := false
	for _, app := range applications {
		if app.Name == name {
			validEntryFound = true

			// Validate application name format
			err := validateServiceName(serviceName)
			if err != nil {
				return fmt.Errorf("failure parsing application name")
			}

			// Validate application args
			if appArgs != nil {
				err = validateApplicationArgs(appArgs, app.Args, app.Directories)
				if err != nil {
					return err
				}
			}
		}
	}
	if !validEntryFound {
		return fmt.Errorf("application not found in manifest")
	}

	return nil
}
