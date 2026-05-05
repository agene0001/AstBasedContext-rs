package main

import "fmt"

// Check 1: Passthrough wrapper
func GetUser(id int) (*User, error) {
	return fetchUserFromDB(id)
}

func fetchUserFromDB(id int) (*User, error) {
	db := connectToDatabase()
	row := db.QueryRow("SELECT * FROM users WHERE id = ?", id)
	user := &User{}
	err := row.Scan(&user.Name, &user.Email)
	if err != nil {
		return nil, fmt.Errorf("user %d not found: %w", id, err)
	}
	return user, nil
}

// Check 2: Near-duplicates
func FormatUserReport(user *User) string {
	result := fmt.Sprintf("Name: %s\n", user.Name)
	result += fmt.Sprintf("Email: %s\n", user.Email)
	result += fmt.Sprintf("Role: %s\n", user.Role)
	result += fmt.Sprintf("Active: %v\n", user.Active)
	result += fmt.Sprintf("Created: %s\n", user.CreatedAt)
	return result
}

func FormatCustomerReport(customer *Customer) string {
	result := fmt.Sprintf("Name: %s\n", customer.Name)
	result += fmt.Sprintf("Email: %s\n", customer.Email)
	result += fmt.Sprintf("Role: %s\n", customer.Role)
	result += fmt.Sprintf("Active: %v\n", customer.Active)
	result += fmt.Sprintf("Created: %s\n", customer.CreatedAt)
	return result
}

// Check 92: Slice used as set
func CollectUniqueTags(items []Item) []string {
	tags := []string{}
	for _, item := range items {
		for _, tag := range item.Tags {
			found := false
			for _, existing := range tags {
				if existing == tag {
					found = true
					break
				}
			}
			if !found {
				tags = append(tags, tag)
			}
		}
	}
	return tags
}

// Check 97: Nested loop lookup
func FindMatchingPairs(listA []Entry, listB []Entry) []Match {
	matches := []Match{}
	for _, a := range listA {
		for _, b := range listB {
			if a.ID == b.ID {
				matches = append(matches, Match{A: a, B: b})
			}
		}
	}
	return matches
}

// Check 95: String concat in loop
func BuildReport(entries []LogEntry) string {
	output := ""
	for _, e := range entries {
		output += fmt.Sprintf("[%s] %s: %s\n", e.Level, e.Timestamp, e.Message)
	}
	return output
}
