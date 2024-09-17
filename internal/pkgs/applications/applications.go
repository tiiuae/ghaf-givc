package applications

import (
	"encoding/json"
	"fmt"
	"givc/internal/pkgs/types"
	"givc/internal/pkgs/utility"
	"net/url"
	"regexp"
	"strings"

	log "github.com/sirupsen/logrus"

	validation "github.com/go-ozzo/ozzo-validation/v4"
	"github.com/go-ozzo/ozzo-validation/v4/is"
)

func validateServiceName(serviceName string) error {
	return validation.Validate(
		serviceName,
		validation.Required,
		is.PrintableASCII,
		validation.Match(regexp.MustCompile(`^[a-zA-Z0-9_-]+@[a-zA-Z0-9_-]+\.service$`)),
	)
}

func validateUrl(urlString string) error {
	err := validation.Validate(
		urlString,
		validation.Required,
		is.URL,
	)
	if err != nil {
		log.Warnf("Invalid URL in args: %v Error: %v", urlString, err)
		return fmt.Errorf("failure in parsing URL")
	}

	// Disallow some more shenanigans
	reqUrl, err := url.Parse(urlString)
	if err != nil {
		log.Warnf("Invalid URL in args: %v", urlString)
		return fmt.Errorf("failure in parsing URL")
	}
	if reqUrl.Scheme != "https" && reqUrl.Scheme != "http" {
		log.Warnf("Non-HTTP(S) scheme in URL: %v", reqUrl.Scheme)
		return fmt.Errorf("failure in parsing URL")
	}
	if reqUrl.User != nil {
		log.Warnf("User info in URL: %v", reqUrl.User)
		return fmt.Errorf("failure in parsing URL")
	}
	return nil
}

func validateApplicationArgs(args []string, allowedArgs []string) error {

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
		return fmt.Errorf("invalid application argument: %s", arg)
	}
	return nil
}

func ParseApplicationManifests(jsonApplicationString string) ([]types.ApplicationManifest, error) {
	var applications []types.ApplicationManifest

	// Unmarshal JSON string into applications
	err := json.Unmarshal([]byte(jsonApplicationString), &applications)
	if err != nil {
		return nil, fmt.Errorf("error unmarshalling JSON string: %v", err)
	}

	// Verify application manifest formats
	appNames := []string{}
	for _, app := range applications {
		// Check app name not empty
		if app.Name == "" {
			return nil, fmt.Errorf("application name is empty")
		}
		for _, name := range appNames {
			if name == app.Name {
				return nil, fmt.Errorf("duplicate application name")
			}
		}
		appNames = append(appNames, app.Name)

		// Check app command not empty
		if app.Command == "" {
			return nil, fmt.Errorf("application command is empty")
		}

		// Check app args types
		if app.Args != nil {
			for _, argType := range app.Args {
				switch argType {
				case types.APP_ARG_FLAG:
				case types.APP_ARG_URL:
				default:
					return nil, fmt.Errorf("application argument type not supported")
				}
			}
		}
	}
	return applications, nil
}

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
				err = validateApplicationArgs(appArgs, app.Args)
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
