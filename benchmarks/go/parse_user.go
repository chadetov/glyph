package parseuser

import "errors"

type User struct {
	Name string
	Age  int
}

func ParseUser(input interface{}) (User, error) {
	obj, ok := input.(map[string]interface{})
	if !ok {
		return User{}, errors.New("expected object")
	}
	name, ok := obj["name"].(string)
	if !ok {
		return User{}, errors.New("name: expected string")
	}
	age, ok := obj["age"].(float64)
	if !ok {
		return User{}, errors.New("age: expected number")
	}
	return User{Name: name, Age: int(age)}, nil
}
