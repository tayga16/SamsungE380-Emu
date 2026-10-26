import importlib.util
print({name: importlib.util.find_spec(name) is not None for name in ('capstone', 'unicorn', 'PIL', 'numpy')})
