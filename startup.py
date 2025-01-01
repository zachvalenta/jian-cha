import json
import inspect
import sys

import polars as pl
from rich import pretty, traceback, print as rprint
from rich import inspect as ins

import app

pretty.install()
traceback.install(show_locals=False)

###
# EXTERNAL
###

def ri(obj):
    """inspect obj with Rich and show methods"""
    return ins(obj, methods=True)

def chuan(obj):
    """obj inheritance"""
    return obj.__class.__mro__

def dump(arg):
    """dump dictionary with indentation"""
    print(json.dumps(arg, indent=4, sort_keys=True))

###
# STARTUP ITSELF
###

def qiu():
    """view startup module help"""
    rprint(help(__name__))

def vars(*args):
    """view all variables, either in given module or startup module itself"""
    if args:
        rprint(dir(*args))
        for arg in args:
            attributes = {key: getattr(arg, key) for key in dir(arg) if not key.startswith("_")}
            for name, val in attributes.items():
                rprint(f"{name}: {val}")
    else:
        all_variables = {key: value for key, value in globals().items() if not key.startswith("_")}
        rprint(all_variables)

def jk():
    """view user imported modules"""
    all_modules = sys.modules.keys()
    top_level_modules = set(module.split('.')[0] for module in all_modules)
    stdlib_modules = set(sys.stdlib_module_names)
    user_defined_modules = sorted(
        module for module in top_level_modules
        if module not in stdlib_modules and not module.startswith('_')
    )
    rprint(user_defined_modules)

