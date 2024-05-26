// Copyright 2024 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0
package registry

import (
	"fmt"
	"givc/internal/pkgs/types"
	"strconv"
	"strings"
	"sync"

	log "github.com/sirupsen/logrus"
)

type ServiceRegistry struct {
	registry []types.RegistryEntry
	sync.Mutex
}

func NewRegistry() *ServiceRegistry {
	return &ServiceRegistry{}
}

// ADD WATCHER and update reg

func (r *ServiceRegistry) Register(newEntry *types.RegistryEntry) error {

	r.Lock()
	defer r.Unlock()

	for _, entry := range r.registry {
		if entry.Name == newEntry.Name {
			log.Infof("Service %s already in registry, updating...", newEntry.Name)
			r.Deregister(newEntry)
		}
	}
	r.registry = append(r.registry, *newEntry)
	log.Infof("Service %s registered", newEntry.Name)

	return nil
}

func (r *ServiceRegistry) Deregister(oldEntry *types.RegistryEntry) error {

	r.Lock()
	defer r.Unlock()

	regLen := len(r.registry)
	for i, entry := range r.registry {
		if entry.Name == oldEntry.Name {
			r.registry[i] = r.registry[regLen-1]
			r.registry = r.registry[:regLen-1]
			log.Infof("Service %s de-registered", oldEntry.Name)
			return nil
		}
	}

	// @TODO traverse tree and de-register children

	log.Errorf("Service %s not found", oldEntry.Name)
	return fmt.Errorf("element %s not found in registry", oldEntry.Name)
}

func (r *ServiceRegistry) GetEntryByType(entryType types.UnitType) []types.RegistryEntry {

	var entries []types.RegistryEntry
	for _, entry := range r.registry {
		if entry.Type == entryType {
			entries = append(entries, entry)
		}
	}
	if len(entries) < 1 {
		log.Warningf("Service type [%d] not found", entryType)
	}
	return entries
}

func (r *ServiceRegistry) GetEntryByName(name string) *types.RegistryEntry {

	for _, entry := range r.registry {
		if entry.Name == name {
			return &entry
		}
	}
	log.Warningf("Service %s not found", name)
	return nil
}

func (r *ServiceRegistry) GetEntriesByName(name string) []types.RegistryEntry {

	var entries []types.RegistryEntry
	for _, entry := range r.registry {
		if strings.Contains(entry.Name, name) {
			entries = append(entries, entry)
		}
	}
	if len(entries) < 1 {
		log.Warningf("No services named %s not found", name)
	}
	return entries
}

func (r *ServiceRegistry) GetUniqueEntryName(name string) string {

	i := 0
LOOP:
	i += 1
	candidateName := name + "@" + strconv.Itoa(i) + ".service"
	for _, entry := range r.registry {
		if strings.EqualFold(entry.Name, candidateName) {
			goto LOOP
		}
	}
	return candidateName
}

func (r *ServiceRegistry) DebugPrint() {

	log.Infof("Printing registry:")

	for _, entry := range r.registry {
		log.Infof("-Name: %s", entry.Name)
		if entry.Parent != "" {
			log.Infof("---Parent: %s", entry.Parent)
		} else {
			log.Infof("---Parent: None")
		}
		// log.Infof("Address: %s", entry.Addr)
		// log.Infof("Port: %s", entry.Port)
		// log.Infof("Protocol: %s", entry.Protocol)
		// log.Infof("State: %v", entry.State)
		// log.Infof("")
	}
}

func (r *ServiceRegistry) GetWatchList() []*types.RegistryEntry {

	r.Lock()
	defer r.Unlock()

	var watchlist []*types.RegistryEntry
	for _, entry := range r.registry {
		if entry.Watch {
			watchlist = append(watchlist, &entry)
		}
	}

	return watchlist
}
